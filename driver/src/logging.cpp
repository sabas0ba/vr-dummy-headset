#include "logging.h"

#include <chrono>
#include <cstdarg>
#include <cstdio>
#include <mutex>

#include "openvr_driver.h"

namespace vrdummy {

namespace {

std::mutex g_mutex;
LogSink g_sink;
LogLevel g_threshold = LogLevel::Info;

const char* LevelName(LogLevel level) {
    switch (level) {
        case LogLevel::Error: return "ERROR";
        case LogLevel::Warn: return "WARN";
        case LogLevel::Info: return "INFO";
        case LogLevel::Debug: return "DEBUG";
        case LogLevel::Trace: return "TRACE";
    }
    return "?";
}

}  // namespace

void SetLogSink(LogSink sink) {
    std::lock_guard<std::mutex> lock(g_mutex);
    g_sink = std::move(sink);
}

void SetLogThreshold(LogLevel level) {
    std::lock_guard<std::mutex> lock(g_mutex);
    g_threshold = level;
}

LogLevel ParseLogLevel(const std::string& name, LogLevel fallback) {
    if (name == "error") return LogLevel::Error;
    if (name == "warn") return LogLevel::Warn;
    if (name == "info") return LogLevel::Info;
    if (name == "debug") return LogLevel::Debug;
    if (name == "trace") return LogLevel::Trace;
    return fallback;
}

uint64_t MonotonicMicros() {
    using namespace std::chrono;
    return static_cast<uint64_t>(duration_cast<microseconds>(steady_clock::now().time_since_epoch()).count());
}

void Log(LogLevel level, const char* format, ...) {
    char text[1024];
    va_list args;
    va_start(args, format);
    vsnprintf(text, sizeof(text), format, args);
    va_end(args);

    protocol::LogRecord record;
    record.level = level;
    record.timestamp_us = MonotonicMicros();
    record.text = text;

    LogSink sink;
    {
        std::lock_guard<std::mutex> lock(g_mutex);
        if (static_cast<uint8_t>(level) > static_cast<uint8_t>(g_threshold)) return;
        sink = g_sink;
    }

    char line[1100];
    snprintf(line, sizeof(line), "[%s] %s", LevelName(level), text);
    // The accessor dereferences the driver context; it is null before Init
    // and after Cleanup.
    if (vr::VRDriverContext() != nullptr && vr::VRDriverLog() != nullptr) {
        vr::VRDriverLog()->Log(line);
    }
    if (sink) sink(record);
}

}  // namespace vrdummy
