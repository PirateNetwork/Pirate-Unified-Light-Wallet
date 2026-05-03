import Cocoa
import FlutterMacOS

private struct DesktopWindowSizing {
  static let preferredSize = NSSize(width: 1180, height: 760)
  static let preferredMinimumSize = NSSize(width: 860, height: 560)
  static let visibleMargin: CGFloat = 48
  static let maxVisibleFraction: CGFloat = 0.92

  static func initialSize(for visibleSize: NSSize) -> NSSize {
    let availableWidth = availableExtent(visibleSize.width)
    let availableHeight = availableExtent(visibleSize.height)
    if availableWidth <= 0 || availableHeight <= 0 {
      return preferredSize
    }

    var width = min(preferredSize.width, availableWidth)
    var height = min(preferredSize.height, availableHeight)
    let preferredAspect = preferredSize.width / preferredSize.height

    if width / height > preferredAspect {
      width = height * preferredAspect
    } else {
      height = width / preferredAspect
    }

    return NSSize(width: floor(width), height: floor(height))
  }

  static func minimumSize(for initialSize: NSSize) -> NSSize {
    let width = min(
      initialSize.width,
      max(640, min(preferredMinimumSize.width, initialSize.width))
    )
    let height = min(
      initialSize.height,
      max(480, min(preferredMinimumSize.height, initialSize.height))
    )

    NSSize(
      width: floor(width),
      height: floor(height)
    )
  }

  private static func availableExtent(_ visibleExtent: CGFloat) -> CGFloat {
    if visibleExtent <= 0 {
      return 0
    }

    let insetExtent = visibleExtent - visibleMargin
    let proportionalExtent = visibleExtent * maxVisibleFraction
    return max(320, min(insetExtent, proportionalExtent))
  }
}

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    let windowFrame = self.frame
    self.contentViewController = flutterViewController
    self.setFrame(windowFrame, display: true)
    if let screen = self.screen ?? NSScreen.main {
      let initialSize = DesktopWindowSizing.initialSize(
        for: screen.visibleFrame.size
      )
      self.setContentSize(initialSize)
      self.minSize = DesktopWindowSizing.minimumSize(for: initialSize)
      self.center()
    }

    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()
  }
}
