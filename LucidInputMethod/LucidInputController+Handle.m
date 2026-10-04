#import <InputMethodKit/InputMethodKit.h>
#import <objc/runtime.h>

static void LucidNote(NSString *line) {
    NSString *path = @"/tmp/lucid-keys.log";
    NSString *text = [line stringByAppendingString:@"\n"];
    NSFileHandle *handle = [NSFileHandle fileHandleForWritingAtPath:path];
    if (handle == nil) {
        [[NSFileManager defaultManager] createFileAtPath:path contents:[text dataUsingEncoding:NSUTF8StringEncoding] attributes:nil];
        return;
    }
    [handle seekToEndOfFile];
    [handle writeData:[text dataUsingEncoding:NSUTF8StringEncoding]];
    [handle closeFile];
}

static BOOL LucidHandleEvent(id self, SEL _cmd, id event, id client) {
    LucidNote([NSString stringWithFormat:@"handleEvent event=%@ client=%@", event, client]);
    SEL swiftSelector = NSSelectorFromString(@"lucidHandle:client:");
    if (![self respondsToSelector:swiftSelector]) {
        LucidNote(@"missing lucidHandle");
        return NO;
    }
    BOOL (*impl)(id, SEL, id, id) = (BOOL (*)(id, SEL, id, id))[self methodForSelector:swiftSelector];
    return impl(self, swiftSelector, event, client);
}

void LucidInstallHandleEventNow(void) {
    Class cls = NSClassFromString(@"LucidInputController");
    if (cls == Nil) {
        LucidNote(@"class missing");
        return;
    }
    SEL selector = @selector(handleEvent:client:);
    IMP previous = class_replaceMethod(cls, selector, (IMP)LucidHandleEvent, "c@:@@");
    LucidNote([NSString stringWithFormat:@"installed previous=%p responds=%d", previous, [cls instancesRespondToSelector:selector]]);
}
