// Driver logging: writes to IVRDriverLog (vrserver.txt) and forwards each
// record to a sink (the connected feeder) when one is installed.
#pragma once

#include <cstdint>
#include <functional>
#include <string>

#include "protocol.h"

namespace vrdummy {

using LogLevel = protocol::LogLevel;
using LogSink = std::function<void(const protocol::LogRecord&)>;

// Installs a sink that receives every record. Pass an empty function to remove it.
void SetLogSink(LogSink sink);
// Records at a level above this threshold are dropped. Default: Info.
void SetLogThreshold(LogLevel level);
LogLevel ParseLogLevel(const std::string& name, LogLevel fallback);

uint64_t MonotonicMicros();

void Log(LogLevel level, const char* format, ...);

#define VRDUMMY_LOG_ERROR(...) ::vrdummy::Log(::vrdummy::LogLevel::Error, __VA_ARGS__)
#define VRDUMMY_LOG_WARN(...) ::vrdummy::Log(::vrdummy::LogLevel::Warn, __VA_ARGS__)
#define VRDUMMY_LOG_INFO(...) ::vrdummy::Log(::vrdummy::LogLevel::Info, __VA_ARGS__)
#define VRDUMMY_LOG_DEBUG(...) ::vrdummy::Log(::vrdummy::LogLevel::Debug, __VA_ARGS__)
#define VRDUMMY_LOG_TRACE(...) ::vrdummy::Log(::vrdummy::LogLevel::Trace, __VA_ARGS__)

}  // namespace vrdummy
