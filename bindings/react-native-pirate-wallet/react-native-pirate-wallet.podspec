require "json"

package = JSON.parse(File.read(File.join(__dir__, "package.json")))

Pod::Spec.new do |s|
  s.name         = package["name"]
  s.version      = package["version"]
  s.summary      = package["description"]
  s.license      = package["license"]
  s.homepage     = "https://github.com/piratenetwork/Pirate-Unified-Light-Wallet"
  s.authors      = "Pirate Chain Contributors"

  s.platform     = :ios, "15.0"
  s.source       = { :path => "." }
  s.source_files = "ios/PirateWalletReactNative.m"
  s.vendored_frameworks = "ios/Frameworks/PirateWalletNative.xcframework"

  s.dependency "React-Core"
end
