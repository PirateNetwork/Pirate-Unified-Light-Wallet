#import <React/RCTBridgeModule.h>

@interface RCT_EXTERN_MODULE(PirateWalletReactNative, NSObject)

RCT_EXTERN_METHOD(invoke:(NSString *)requestJson
                  pretty:(BOOL)pretty
                  resolver:(RCTPromiseResolveBlock)resolve
                  rejecter:(RCTPromiseRejectBlock)reject)

+ (BOOL)requiresMainQueueSetup
{
  return NO;
}

@end
