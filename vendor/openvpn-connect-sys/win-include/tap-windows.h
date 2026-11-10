/* Krunk (vpn/vendor, from the 0.14.0 VPN spike): minimal stand-in for tap-windows6's tap-windows.h (GPL-2.0, not vendored).
 * OpenVPN 3 Core's tun/win/tunutil.hpp needs these interface constants to compile on Windows.
 * Krunk uses OpenVPN's external-tun (no TAP/Wintun/DCO driver), so this code path is never run.
 * The values are the public TAP-Windows driver ABI (device type, IOCTL numbers, registry keys). */
#ifndef KRUNK_TAP_WINDOWS_STUB_H
#define KRUNK_TAP_WINDOWS_STUB_H

#include <winioctl.h>

#define TAP_WIN_CONTROL_CODE(request, method) \
    CTL_CODE(FILE_DEVICE_UNKNOWN, request, method, FILE_ANY_ACCESS)

#define TAP_WIN_IOCTL_GET_MAC TAP_WIN_CONTROL_CODE(1, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_GET_VERSION TAP_WIN_CONTROL_CODE(2, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_GET_MTU TAP_WIN_CONTROL_CODE(3, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_GET_INFO TAP_WIN_CONTROL_CODE(4, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_CONFIG_POINT_TO_POINT TAP_WIN_CONTROL_CODE(5, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_SET_MEDIA_STATUS TAP_WIN_CONTROL_CODE(6, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_CONFIG_DHCP_MASQ TAP_WIN_CONTROL_CODE(7, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_GET_LOG_LINE TAP_WIN_CONTROL_CODE(8, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_CONFIG_DHCP_SET_OPT TAP_WIN_CONTROL_CODE(9, METHOD_BUFFERED)
#define TAP_WIN_IOCTL_CONFIG_TUN TAP_WIN_CONTROL_CODE(10, METHOD_BUFFERED)

#define ADAPTER_KEY "SYSTEM\\CurrentControlSet\\Control\\Class\\{4D36E972-E325-11CE-BFC1-08002BE10318}"
#define NETWORK_CONNECTIONS_KEY "SYSTEM\\CurrentControlSet\\Control\\Network\\{4D36E972-E325-11CE-BFC1-08002BE10318}"
#define USERMODEDEVICEDIR "\\\\.\\Global\\"
#define SYSDEVICEDIR "\\Device\\"
#define USERDEVICEDIR "\\DosDevices\\Global\\"
#define TAP_WIN_SUFFIX ".tap"

#endif
