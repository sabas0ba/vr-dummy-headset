// Named pipe server. The driver is the server; the feeder connects.
// One client at a time. Frames are delimited by the protocol header.
#pragma once

#include <atomic>
#include <cstdint>
#include <functional>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

#include "protocol.h"

namespace vrdummy {

struct PipeHandler {
    std::function<void()> on_connected;
    std::function<void()> on_disconnected;
    std::function<void(const protocol::Message&)> on_message;
};

class PipeServer {
public:
    PipeServer() = default;
    ~PipeServer();
    PipeServer(const PipeServer&) = delete;
    PipeServer& operator=(const PipeServer&) = delete;

    // pipe_name is the name without the \\.\pipe\ prefix.
    bool Start(const std::string& pipe_name, PipeHandler handler);
    void Stop();

    // Thread-safe. Returns false when no client is connected or the write fails.
    // Never logs, so it is safe to call from a log sink.
    bool Send(const std::vector<uint8_t>& frame);
    bool connected() const { return connected_.load(); }

private:
    void Run();
    bool ServeOneClient();

    std::string path_;
    PipeHandler handler_;
    std::thread thread_;
    std::atomic<bool> running_{false};
    std::atomic<bool> connected_{false};
    std::mutex write_mutex_;
    void* pipe_ = nullptr;        // HANDLE
    void* stop_event_ = nullptr;  // HANDLE
};

}  // namespace vrdummy
