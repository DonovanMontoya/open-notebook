// The Rust app's C interface (apps/mac/app/src/ios.rs). All calls are main-thread only.
#include <stdbool.h>
#include <stdint.h>

typedef struct OnHost OnHost;

typedef struct {
    bool animating;
    bool wants_keyboard;
    double wake_after;
} OnFrame;

typedef void (*OnWake)(void *ctx);

OnHost *on_create(void *layer, float width, float height, float scale, bool dark, const char *data_dir, OnWake wake, void *ctx);
OnFrame on_frame(OnHost *h);
void on_resize(OnHost *h, float width, float height, float scale);
void on_touch(OnHost *h, uint64_t id, int32_t phase, float x, float y, double time);
void on_text(OnHost *h, const char *text);
void on_backspace(OnHost *h);
void on_fold(OnHost *h, float x, float y, float width, float height, bool active);
void on_inset_bottom(OnHost *h, float bottom);
void on_destroy(OnHost *h);
