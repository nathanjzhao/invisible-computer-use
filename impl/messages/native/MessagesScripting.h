// Interoperability declarations for the public Messages.sdef scripting vocabulary.
#import <AppKit/AppKit.h>
#import <ScriptingBridge/ScriptingBridge.h>
@class CUMAccount,CUMChat,CUMParticipant;
@interface CUMApplication : SBApplication
- (SBElementArray *)accounts;
- (SBElementArray *)chats;
- (void)send:(id)payload to:(id)destination;
@end
@interface CUMAccount : SBObject
- (NSString *)id;
- (BOOL)enabled;
- (OSType)connectionStatus;
- (OSType)serviceType;
- (SBElementArray *)participants;
- (SBElementArray *)chats;
@end
@interface CUMParticipant : SBObject
- (NSString *)id;
- (NSString *)handle;
@end
@interface CUMChat : SBObject
- (NSString *)id;
- (CUMAccount *)account;
- (SBElementArray *)participants;
@end
