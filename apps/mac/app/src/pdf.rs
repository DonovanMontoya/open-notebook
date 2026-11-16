//! Original PDF pages embedded in Library's content region.
use notebook_engine::types::SourceFile;

pub fn is_pdf(file: &SourceFile) -> bool {
    file.mime.eq_ignore_ascii_case("application/pdf") || file.name.to_ascii_lowercase().ends_with(".pdf") || file.data.starts_with(b"%PDF-")
}

#[cfg(target_os = "macos")]
mod native {
    use std::{
        ffi::{CStr, CString, c_char, c_void},
        hash::{DefaultHasher, Hash, Hasher},
        io,
        os::unix::fs::{DirBuilderExt, PermissionsExt},
        sync::OnceLock,
    };
    type Update = unsafe extern "C" fn(*const u8, usize, u64, f64, f64, f64, f64) -> i32;
    type Pages = unsafe extern "C" fn(*const u8, usize) -> i32;
    type Event = unsafe extern "C" fn(*mut u8, usize) -> usize;
    type Navigate = unsafe extern "C" fn(i32, *const u8, usize);
    type Hide = unsafe extern "C" fn();
    type Visualization = unsafe extern "C" fn(*const u8, usize, bool, bool, u64);
    type VisualizationPage = unsafe extern "C" fn() -> i32;
    unsafe extern "C" {
        fn dlopen(path: *const c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
        fn dlerror() -> *const c_char;
    }
    struct Bridge {
        update: Update,
        pages: Pages,
        hide: Hide,
        event: Event,
        navigate: Navigate,
        visualization: Visualization,
        visualization_page: VisualizationPage,
    }
    static BRIDGE: OnceLock<Result<Bridge, String>> = OnceLock::new();
    fn bridge() -> io::Result<&'static Bridge> {
        BRIDGE
            .get_or_init(|| {
                let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
                let dir = std::env::temp_dir().join(format!("notebook-pdf-{}-{unique}", std::process::id()));
                let load = || -> io::Result<Bridge> {
                    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
                    let path = dir.join("libNotebookPDF.dylib");
                    std::fs::write(&path, include_bytes!(concat!(env!("OUT_DIR"), "/libNotebookPDF.dylib")))?;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500))?;
                    let path = CString::new(path.as_os_str().as_encoded_bytes())?;
                    // The embedded module stays loaded for the process lifetime; its native views
                    // and callbacks must never outlive their Swift code.
                    unsafe {
                        let handle = dlopen(path.as_ptr(), 2);
                        if handle.is_null() {
                            let error = dlerror();
                            return Err(io::Error::other(if error.is_null() {
                                "Cannot load PDF renderer".into()
                            } else {
                                CStr::from_ptr(error).to_string_lossy().into_owned()
                            }));
                        }
                        let update = dlsym(handle, c"notebook_pdf_update".as_ptr());
                        let pages = dlsym(handle, c"notebook_pdf_pages".as_ptr());
                        let event = dlsym(handle, c"notebook_pdf_event".as_ptr());
                        let navigate = dlsym(handle, c"notebook_pdf_navigate".as_ptr());
                        let hide = dlsym(handle, c"notebook_pdf_hide".as_ptr());
                        let visualization = dlsym(handle, c"notebook_pdf_visualization".as_ptr());
                        let visualization_page = dlsym(handle, c"notebook_pdf_visualization_page".as_ptr());
                        if update.is_null()
                            || pages.is_null()
                            || hide.is_null()
                            || event.is_null()
                            || navigate.is_null()
                            || visualization.is_null()
                            || visualization_page.is_null()
                        {
                            return Err(io::Error::other("PDF renderer entrypoint missing"));
                        }
                        Ok(Bridge {
                            update: std::mem::transmute::<*mut c_void, Update>(update),
                            pages: std::mem::transmute::<*mut c_void, Pages>(pages),
                            event: std::mem::transmute::<*mut c_void, Event>(event),
                            navigate: std::mem::transmute::<*mut c_void, Navigate>(navigate),
                            visualization: std::mem::transmute::<*mut c_void, Visualization>(visualization),
                            visualization_page: std::mem::transmute::<*mut c_void, VisualizationPage>(visualization_page),
                            hide: std::mem::transmute::<*mut c_void, Hide>(hide),
                        })
                    }
                };
                let result = load().map_err(|e| e.to_string());
                let _ = std::fs::remove_dir_all(dir);
                result
            })
            .as_ref()
            .map_err(|e| io::Error::other(e.clone()))
    }
    pub struct OriginalPdf {
        data: Vec<u8>,
        id: u64,
    }
    impl OriginalPdf {
        pub fn prepare(file: &super::SourceFile) -> io::Result<Self> {
            let api = bridge()?;
            if unsafe { (api.pages)(file.data.as_ptr(), file.data.len()) } <= 0 {
                return Err(io::Error::other("The original PDF could not be opened or is locked."));
            }
            let mut hash = DefaultHasher::new();
            file.data.hash(&mut hash);
            Ok(Self { data: file.data.clone(), id: hash.finish() })
        }
        /// Called only by Rune's layout handler on the Mac UI thread.
        pub fn show(&self, [x, y, w, h]: [f32; 4]) -> io::Result<()> {
            let api = bridge()?;
            let shown = unsafe { (api.update)(self.data.as_ptr(), self.data.len(), self.id, x as f64, y as f64, w as f64, h as f64) };
            if shown == 0 {
                return Err(io::Error::other("Could not embed the original PDF in Library."));
            }
            Ok(())
        }
    }
    pub fn next_event() -> Option<crate::reading::ReadingContext> {
        let api = BRIDGE.get()?.as_ref().ok()?;
        let size = unsafe { (api.event)(std::ptr::null_mut(), 0) };
        if size == 0 || size > 16 * 1024 * 1024 {
            return None;
        }
        let mut bytes = vec![0; size];
        unsafe {
            (api.event)(bytes.as_mut_ptr(), bytes.len());
        }
        serde_json::from_slice(&bytes).ok()
    }
    pub fn navigate(page: u32, text: &str) {
        if let Some(Ok(api)) = BRIDGE.get() {
            unsafe { (api.navigate)(page as i32, text.as_ptr(), text.len()) }
        }
    }
    pub fn show_visualization(html: Option<&str>, dark: bool, revision: u64) {
        if let Some(Ok(api)) = BRIDGE.get() {
            let body = html.unwrap_or("");
            unsafe { (api.visualization)(body.as_ptr(), body.len(), html.is_some(), dark, revision) }
        }
    }
    pub fn visualization_page() -> Option<u32> {
        let api = BRIDGE.get()?.as_ref().ok()?;
        let page = unsafe { (api.visualization_page)() };
        (page > 0).then_some(page as u32)
    }
    pub fn hide() {
        if let Some(Ok(api)) = BRIDGE.get() {
            unsafe { (api.hide)() }
        }
    }
    #[cfg(test)]
    mod tests {
        #[test]
        fn visualization_envelope_preserves_content_without_host_script_injection() {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            let executable = std::env::temp_dir().join(format!("notebook-html-test-{}", std::process::id()));
            let result = std::process::Command::new("xcrun")
                .arg("swiftc")
                .arg(root.join("native/FigureHTML.swift"))
                .arg(root.join("native/tests/FigureHTMLTests.swift"))
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
            let result = std::process::Command::new(&executable).output().unwrap();
            let _ = std::fs::remove_file(executable);
            assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        }
        #[test]
        fn original_pages_are_preserved_without_a_reader_process() {
            let data = include_bytes!("../tests/fixtures/original.pdf").to_vec();
            let file = super::super::SourceFile { name: "paper.pdf".into(), mime: "application/pdf".into(), data: data.clone() };
            let pdf = super::OriginalPdf::prepare(&file).unwrap();
            assert_eq!(pdf.data, data);
            assert_eq!(unsafe { (super::bridge().unwrap().pages)(pdf.data.as_ptr(), pdf.data.len()) }, 2);
            let corrupt = super::super::SourceFile { data: b"not a PDF".to_vec(), ..file };
            assert!(super::OriginalPdf::prepare(&corrupt).is_err());
        }
    }
}
#[cfg(target_os = "macos")]
pub use native::{OriginalPdf, hide, navigate, next_event, show_visualization, visualization_page};

#[cfg(test)]
mod tests {
    #[test]
    fn pdf_detection_uses_metadata_and_signature() {
        let mut file = super::SourceFile { name: "paper.PDF".into(), mime: String::new(), data: Vec::new() };
        assert!(super::is_pdf(&file));
        file.name = "download".into();
        file.data = b"%PDF-1.4".to_vec();
        assert!(super::is_pdf(&file));
        file.data.clear();
        assert!(!super::is_pdf(&file));
    }
}
