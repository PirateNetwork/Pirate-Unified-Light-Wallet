#include <flutter/dart_project.h>
#include <flutter/flutter_view_controller.h>
#include <flutter_windows.h>
#include <cmath>
#include <windows.h>

#include "flutter_window.h"
#include "utils.h"

namespace {
constexpr double kPreferredWindowWidth = 1180.0;
constexpr double kPreferredWindowHeight = 760.0;
constexpr double kVisibleMargin = 48.0;
constexpr double kMaxVisibleFraction = 0.92;

double AvailableExtent(double visible_extent) {
  if (visible_extent <= 0) {
    return 0;
  }
  const double inset_extent = visible_extent - kVisibleMargin;
  const double proportional_extent = visible_extent * kMaxVisibleFraction;
  const double limited_extent =
      inset_extent < proportional_extent ? inset_extent : proportional_extent;
  return limited_extent > 320.0 ? limited_extent : 320.0;
}

Win32Window::Size ResolveInitialWindowSize(const Win32Window::Point& origin) {
  const POINT target_point = {static_cast<LONG>(origin.x),
                              static_cast<LONG>(origin.y)};
  HMONITOR monitor = MonitorFromPoint(target_point, MONITOR_DEFAULTTONEAREST);

  MONITORINFO monitor_info;
  monitor_info.cbSize = sizeof(MONITORINFO);
  RECT work_area = {0, 0, 1920, 1080};
  if (GetMonitorInfo(monitor, &monitor_info)) {
    work_area = monitor_info.rcWork;
  }

  const UINT dpi = FlutterDesktopGetDpiForMonitor(monitor);
  const double scale_factor = dpi > 0 ? dpi / 96.0 : 1.0;
  const double visible_width =
      (work_area.right - work_area.left) / scale_factor;
  const double visible_height =
      (work_area.bottom - work_area.top) / scale_factor;

  const double available_width = AvailableExtent(visible_width);
  const double available_height = AvailableExtent(visible_height);
  double width = kPreferredWindowWidth < available_width
                     ? kPreferredWindowWidth
                     : available_width;
  double height =
      kPreferredWindowHeight < available_height ? kPreferredWindowHeight
                                                : available_height;
  constexpr double preferred_aspect =
      kPreferredWindowWidth / kPreferredWindowHeight;

  if (width / height > preferred_aspect) {
    width = height * preferred_aspect;
  } else {
    height = width / preferred_aspect;
  }

  return Win32Window::Size(static_cast<unsigned int>(std::floor(width)),
                           static_cast<unsigned int>(std::floor(height)));
}
}  // namespace

int APIENTRY wWinMain(_In_ HINSTANCE instance, _In_opt_ HINSTANCE prev,
                      _In_ wchar_t *command_line, _In_ int show_command) {
  // Attach to console when present (e.g., 'flutter run') or create a
  // new console when running with a debugger.
  if (!::AttachConsole(ATTACH_PARENT_PROCESS) && ::IsDebuggerPresent()) {
    CreateAndAttachConsole();
  }

  // Initialize COM, so that it is available for use in the library and/or
  // plugins.
  ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);

  flutter::DartProject project(L"data");

  std::vector<std::string> command_line_arguments =
      GetCommandLineArguments();

  project.set_dart_entrypoint_arguments(std::move(command_line_arguments));

  FlutterWindow window(project);
  Win32Window::Point origin(10, 10);
  Win32Window::Size size = ResolveInitialWindowSize(origin);
  if (!window.Create(L"app", origin, size)) {
    return EXIT_FAILURE;
  }
  window.SetQuitOnClose(true);

  ::MSG msg;
  while (::GetMessage(&msg, nullptr, 0, 0)) {
    ::TranslateMessage(&msg);
    ::DispatchMessage(&msg);
  }

  ::CoUninitialize();
  return EXIT_SUCCESS;
}
