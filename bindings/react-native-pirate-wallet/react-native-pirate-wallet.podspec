require "json"

package = JSON.parse(File.read(File.join(__dir__, "package.json")))

assembly_script = File.join(__dir__, "scripts", "assemble-ios-framework.js")
framework_path = File.join(__dir__, "ios", "Frameworks", "PirateWalletNative.xcframework")

# npm may block dependency lifecycle scripts, so assemble the split iOS binary
# packages when CocoaPods actually needs the framework.
Pod::Executable.execute_command("node", [assembly_script, "--force"])
unless File.directory?(framework_path)
  raise Pod::Informative, "PirateWalletNative.xcframework could not be assembled"
end

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
  s.frameworks = "Security"

  s.dependency "React-Core"
end
