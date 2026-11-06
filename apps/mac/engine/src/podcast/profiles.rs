//! Default podcast profiles, copied from the Open Notebook server so episodes are
//! styled the same in both modes. Port of `profile-seeds.ts`.

use crate::types::Speaker;

pub struct SpeakerProfileSeed {
    pub name: &'static str,
    pub description: &'static str,
    pub voice_kind: &'static str,
    pub voice_model: &'static str,
    /// (name, voice, backstory, personality)
    pub speakers: &'static [(&'static str, &'static str, &'static str, &'static str)],
}

impl SpeakerProfileSeed {
    pub fn speakers(&self) -> Vec<Speaker> {
        self.speakers
            .iter()
            .map(|(name, voice, backstory, personality)| Speaker {
                name: (*name).into(),
                voice_id: (*voice).into(),
                backstory: (*backstory).into(),
                personality: (*personality).into(),
            })
            .collect()
    }
}

pub struct EpisodeProfileSeed {
    pub name: &'static str,
    pub description: &'static str,
    pub speaker_profile_name: &'static str,
    pub briefing: &'static str,
    pub segments: u32,
}

pub const DEFAULT_SPEAKER_PROFILES: [SpeakerProfileSeed; 3] = [
    SpeakerProfileSeed {
        name: "business_panel",
        description: "Business analysis panel with diverse perspectives",
        voice_kind: "openai",
        voice_model: "gpt-4o-mini-tts",
        speakers: &[
            ("Marcus Thompson", "echo", "Former McKinsey consultant, now startup advisor. Expert in strategic analysis and market dynamics.", "Strategic thinker, data-driven, excellent at identifying key insights and implications"),
            ("Elena Vasquez", "shimmer", "Serial entrepreneur and investor. Focuses on practical implementation and execution.", "Action-oriented, pragmatic, brings startup experience and execution focus"),
            ("Johny Bing", "ash", "Youtube celebrity and business mogul. Focuses on practical implementation and execution.", "Controversial, likes to question ideas and concepts. He brings a fresh perspective and always has a point to make."),
        ],
    },
    SpeakerProfileSeed {
        name: "solo_expert",
        description: "Single expert for educational content",
        voice_kind: "openai",
        voice_model: "gpt-4o-mini-tts",
        speakers: &[(
            "Professor Sarah Kim",
            "marin",
            "Distinguished professor and researcher. Has a gift for making complex topics accessible to broad audiences.",
            "Patient teacher, uses analogies and examples, breaks down complex concepts step by step",
        )],
    },
    SpeakerProfileSeed {
        name: "tech_experts",
        description: "Two technical experts for tech discussions",
        voice_kind: "openai",
        voice_model: "gpt-4o-mini-tts",
        speakers: &[
            ("Dr. Alex Chen", "marin", "Senior AI researcher and former tech lead at major companies. Specializes in making complex technical concepts accessible.", "Analytical, clear communicator, asks probing questions to dig deeper into technical details"),
            ("Jamie Rodriguez", "alloy", "Full-stack engineer and tech entrepreneur. Loves practical applications and real-world implementations.", "Enthusiastic, practical-minded, great at explaining implementation details and trade-offs"),
        ],
    },
];

pub const DEFAULT_EPISODE_PROFILES: [EpisodeProfileSeed; 3] = [
    EpisodeProfileSeed {
        name: "business_analysis",
        description: "Business-focused analysis and discussion",
        speaker_profile_name: "business_panel",
        briefing: "Analyze the provided content from a business perspective. Discuss market implications, strategic insights, competitive advantages, and actionable business intelligence.",
        segments: 6,
    },
    EpisodeProfileSeed {
        name: "solo_expert",
        description: "Single expert explaining complex topics",
        speaker_profile_name: "solo_expert",
        briefing: "Create an educational explanation of the provided content. Break down complex concepts into digestible segments, use analogies and examples, and maintain an engaging teaching style.",
        segments: 4,
    },
    EpisodeProfileSeed {
        name: "tech_discussion",
        description: "Technical discussion between 2 experts",
        speaker_profile_name: "tech_experts",
        briefing: "Create an engaging technical discussion about the provided content. Focus on practical insights, real-world applications, and detailed explanations that would interest developers and technical professionals.",
        segments: 5,
    },
];
