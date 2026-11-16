// macOS-only, opt-in diagnostics. Never log text or keystrokes.
// Linked into the host, not the reloadable UI dylib: swizzled IMPs must stay
// valid for the entire process even after a hot reload.
#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#import <fcntl.h>
#import <unistd.h>

static int log_fd = -1;
static IMP original_insert, original_mark, original_unmark, original_command;
static IMP original_activate, original_deactivate, original_discard;

static void record(NSString *event, id object) {
    if (log_fd < 0) return;
    static NSDateFormatter *formatter;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        formatter = [NSDateFormatter new];
        formatter.locale = [NSLocale localeWithLocaleIdentifier:@"en_US_POSIX"];
        formatter.timeZone = [NSTimeZone timeZoneForSecondsFromGMT:0];
        formatter.dateFormat = @"yyyy-MM-dd'T'HH:mm:ss.SSS'Z'";
    });
    NSWindow *window = NSApp.keyWindow;
    NSString *line = [NSString stringWithFormat:@"%@ pid=%d %@ object=%p key_window=%p first_responder=%p modifiers=0x%lx\n",
                      [formatter stringFromDate:[NSDate date]], getpid(), event,
                      (__bridge void *)object, (__bridge void *)window,
                      (__bridge void *)window.firstResponder, (unsigned long)[NSEvent modifierFlags]];
    const char *bytes = line.UTF8String;
    if (bytes) (void)write(log_fd, bytes, strlen(bytes));
}
static void insert_text(id self, SEL selector, id text, NSRange replacement) {
    record(@"insertText", self);
    ((void (*)(id, SEL, id, NSRange))original_insert)(self, selector, text, replacement);
}
static void set_marked(id self, SEL selector, id text, NSRange selected, NSRange replacement) {
    record(@"setMarkedText", self);
    ((void (*)(id, SEL, id, NSRange, NSRange))original_mark)(self, selector, text, selected, replacement);
}
static void unmark(id self, SEL selector) {
    record(@"unmarkText", self);
    ((void (*)(id, SEL))original_unmark)(self, selector);
}
static void command(id self, SEL selector, SEL action) {
    record([NSString stringWithFormat:@"doCommandBySelector selector=%s", sel_getName(action)], self);
    ((void (*)(id, SEL, SEL))original_command)(self, selector, action);
}
static void activate(id self, SEL selector) {
    record(@"context-activate", self);
    ((void (*)(id, SEL))original_activate)(self, selector);
}
static void deactivate(id self, SEL selector) {
    record(@"context-deactivate", self);
    ((void (*)(id, SEL))original_deactivate)(self, selector);
}
static void discard(id self, SEL selector) {
    record(@"context-discardMarkedText", self);
    ((void (*)(id, SEL))original_discard)(self, selector);
}
static BOOL hook(Class cls, SEL selector, IMP replacement, IMP *original) {
    Method method = class_getInstanceMethod(cls, selector);
    if (!method) { record([NSString stringWithFormat:@"hook-missing class=%s selector=%s", class_getName(cls), sel_getName(selector)], nil); return NO; }
    *original = method_getImplementation(method);
    class_replaceMethod(cls, selector, replacement, method_getTypeEncoding(method));
    return YES;
}
static void install_view_hooks(void) {
    static BOOL installed = NO;
    if (installed) return;
    Class view = NSClassFromString(@"GPUIView");
    if (!view) return;
    // GPUIView declares these methods. A missing selector is reported and is
    // not replaced, preserving normal behavior on a future GPUI revision.
    hook(view, @selector(insertText:replacementRange:), (IMP)insert_text, &original_insert);
    hook(view, @selector(setMarkedText:selectedRange:replacementRange:), (IMP)set_marked, &original_mark);
    hook(view, @selector(unmarkText), (IMP)unmark, &original_unmark);
    hook(view, @selector(doCommandBySelector:), (IMP)command, &original_command);
    installed = YES;
    record(@"view-hooks-installed", nil);
}
void jcode_ime_log_start(void) {
    const char *enabled = getenv("JCODE_DESKTOP_IME_LOG");
    if (!enabled || strcmp(enabled, "1") != 0) return;
    @autoreleasepool {
    NSString *directory = [NSHomeDirectory() stringByAppendingPathComponent:@".jcode/logs"];
    [[NSFileManager defaultManager] createDirectoryAtPath:directory withIntermediateDirectories:YES attributes:@{NSFilePosixPermissions: @0700} error:nil];
    NSString *path = [directory stringByAppendingPathComponent:@"input-source.log"];
    log_fd = open(path.fileSystemRepresentation, O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (log_fd < 0) return;
    record(@"desktop-recorder-started", nil);
    Class context = [NSTextInputContext class];
    hook(context, @selector(activate), (IMP)activate, &original_activate);
    hook(context, @selector(deactivate), (IMP)deactivate, &original_deactivate);
    hook(context, @selector(discardMarkedText), (IMP)discard, &original_discard);
    NSNotificationCenter *center = [NSNotificationCenter defaultCenter];
    [center addObserverForName:NSWindowDidBecomeKeyNotification object:nil queue:[NSOperationQueue mainQueue] usingBlock:^(NSNotification *note) {
        install_view_hooks(); record(@"window-become-key", note.object);
    }];
    [center addObserverForName:NSWindowDidResignKeyNotification object:nil queue:[NSOperationQueue mainQueue] usingBlock:^(NSNotification *note) {
        record(@"window-resign-key", note.object);
    }];
    install_view_hooks();
    }
}
