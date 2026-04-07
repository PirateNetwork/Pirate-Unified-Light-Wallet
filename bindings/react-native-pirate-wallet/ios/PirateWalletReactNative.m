#import <React/RCTBridgeModule.h>
@import PirateWalletNative;

@interface PirateWalletReactNative : NSObject <RCTBridgeModule>
@end

@implementation PirateWalletReactNative

RCT_EXPORT_MODULE();

+ (BOOL)requiresMainQueueSetup
{
  return NO;
}

RCT_REMAP_METHOD(invoke,
                 invoke:(NSString *)requestJson
                 pretty:(BOOL)pretty
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject)
{
  const char *requestCString = [requestJson UTF8String];
  if (requestCString == NULL) {
    reject(@"PIRATE_WALLET_INVOKE_ERROR", @"Request string was not valid UTF-8.", nil);
    return;
  }

  char *responsePtr = pirate_wallet_service_invoke_json(requestCString, pretty);
  if (responsePtr == NULL) {
    reject(@"PIRATE_WALLET_INVOKE_ERROR", @"Wallet service returned a null response.", nil);
    return;
  }

  NSString *response = [NSString stringWithUTF8String:responsePtr];
  pirate_wallet_service_free_string(responsePtr);

  if (response == nil) {
    reject(@"PIRATE_WALLET_INVOKE_ERROR", @"Wallet service returned invalid UTF-8.", nil);
    return;
  }

  resolve(response);
}

@end
