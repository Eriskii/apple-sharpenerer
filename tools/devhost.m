// Development host: loads the payload with dlopen, opens test windows at known spots and saves a
// capture of each (with shadow), optionally after pushing a new packed config through the live
// update path.
//
//   devhost <libsharpenerer.dylib> <out-dir> [packed-config-hex]
#import <AppKit/AppKit.h>
#import <dlfcn.h>
#import <notify.h>

static void capture(NSWindow *w, NSString *path) {
    typedef CGImageRef (*Capture)(CGRect, uint32_t, uint32_t, uint32_t);
    Capture create = (Capture)dlsym(RTLD_DEFAULT, "CGWindowListCreateImage");
    CGImageRef image = create(CGRectNull, 1 << 3 /* including window */, (uint32_t)w.windowNumber, 1 << 3 /* best resolution */);
    if (!image) { fprintf(stderr, "capture of %s failed\n", path.UTF8String); return; }
    NSData *png = [[[NSBitmapImageRep alloc] initWithCGImage:image] representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    [png writeToFile:path atomically:YES];
    CGImageRelease(image);
}

static void spin(NSTimeInterval seconds) {
    [[NSRunLoop mainRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:seconds]];
}

int main(int argc, char **argv) {
    @autoreleasepool {
        if (argc < 3) { fprintf(stderr, "usage: devhost <payload> <out-dir> [config-hex]\n"); return 2; }
        NSString *out = @(argv[2]);
        [NSFileManager.defaultManager createDirectoryAtPath:out withIntermediateDirectories:YES attributes:nil error:nil];
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        if (!dlopen(argv[1], RTLD_NOW)) { fprintf(stderr, "dlopen: %s\n", dlerror()); return 1; }

        NSRect vf = NSScreen.mainScreen.visibleFrame;
        CGFloat w = 420, h = 300;
        NSDictionary<NSString *, NSValue *> *spots = @{
            @"top-left-flush": [NSValue valueWithRect:NSMakeRect(NSMinX(vf), NSMaxY(vf) - h, w, h)],
            @"top-right-8pt": [NSValue valueWithRect:NSMakeRect(NSMaxX(vf) - 8 - w, NSMaxY(vf) - 8 - h, w, h)],
            @"center": [NSValue valueWithRect:NSMakeRect(NSMidX(vf) - w / 2, NSMidY(vf) - h / 2, w, h)],
            @"bottom-left-flush": [NSValue valueWithRect:NSMakeRect(NSMinX(vf), NSMinY(vf), w, h)],
        };
        NSMutableDictionary<NSString *, NSWindow *> *windows = [NSMutableDictionary dictionary];
        for (NSString *name in spots) {
            NSWindow *win = [[NSWindow alloc] initWithContentRect:NSZeroRect
                                                        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable
                                                          backing:NSBackingStoreBuffered defer:NO];
            win.releasedWhenClosed = NO;
            win.title = name;
            win.contentView.wantsLayer = YES;
            win.contentView.layer.backgroundColor = NSColor.systemTealColor.CGColor;
            [win setFrame:spots[name].rectValue display:YES];
            [win orderFrontRegardless];
            windows[name] = win;
        }
        spin(1.0);
        for (NSString *name in windows) {
            NSWindow *win = windows[name];
            printf("%-18s frame %s\n", name.UTF8String, NSStringFromRect(win.frame).UTF8String);
            capture(win, [out stringByAppendingFormat:@"/%@.png", name]);
        }

        if (argc > 3) {
            uint64_t packed = strtoull(argv[3], NULL, 16);
            int token;
            notify_register_check("io.github.eriskii.sharpenerer.config", &token);
            notify_set_state(token, packed);
            notify_post("io.github.eriskii.sharpenerer.config");
            spin(1.0);
            for (NSString *name in windows) capture(windows[name], [out stringByAppendingFormat:@"/%@-live.png", name]);
            notify_set_state(token, 0);
            notify_cancel(token);
        }
    }
    return 0;
}
