#include "pipe_server.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include "logging.h"

namespace vrdummy {

namespace {

constexpr DWORD kBufferSize = 64 * 1024;

class OverlappedEvent {
public:
    OverlappedEvent() : event_(CreateEventW(nullptr, TRUE, FALSE, nullptr)) {}
    ~OverlappedEvent() {
        if (event_ != nullptr) CloseHandle(event_);
    }
    OVERLAPPED* Reset() {
        overlapped_ = {};
        overlapped_.hEvent = event_;
        ResetEvent(event_);
        return &overlapped_;
    }
    HANDLE event() const { return event_; }
    bool valid() const { return event_ != nullptr; }

private:
    HANDLE event_ = nullptr;
    OVERLAPPED overlapped_{};
};

// Waits for an overlapped operation or the stop event.
// Returns true when the operation completed, false on stop or error.
bool WaitOperation(HANDLE pipe, OVERLAPPED* ov, HANDLE stop_event, DWORD* transferred) {
    const HANDLE handles[2] = {ov->hEvent, stop_event};
    const DWORD r = WaitForMultipleObjects(2, handles, FALSE, INFINITE);
    if (r != WAIT_OBJECT_0) {
        CancelIoEx(pipe, ov);
        DWORD ignored = 0;
        GetOverlappedResult(pipe, ov, &ignored, TRUE);
        return false;
    }
    return GetOverlappedResult(pipe, ov, transferred, FALSE) != FALSE;
}

}  // namespace

PipeServer::~PipeServer() { Stop(); }

bool PipeServer::Start(const std::string& pipe_name, PipeHandler handler) {
    if (running_.load()) return false;
    path_ = "\\\\.\\pipe\\" + pipe_name;
    handler_ = std::move(handler);
    stop_event_ = CreateEventW(nullptr, TRUE, FALSE, nullptr);
    if (stop_event_ == nullptr) {
        VRDUMMY_LOG_ERROR("pipe: CreateEvent failed (%lu)", GetLastError());
        return false;
    }
    running_.store(true);
    thread_ = std::thread(&PipeServer::Run, this);
    VRDUMMY_LOG_INFO("pipe: listening on %s", path_.c_str());
    return true;
}

void PipeServer::Stop() {
    if (!running_.exchange(false)) return;
    SetEvent(static_cast<HANDLE>(stop_event_));
    if (thread_.joinable()) thread_.join();
    CloseHandle(static_cast<HANDLE>(stop_event_));
    stop_event_ = nullptr;
}

void PipeServer::Run() {
    while (running_.load()) {
        if (!ServeOneClient()) {
            // Creation failure: back off so that a persistent error does not spin.
            const HANDLE stop = static_cast<HANDLE>(stop_event_);
            if (WaitForSingleObject(stop, 1000) == WAIT_OBJECT_0) break;
        }
    }
}

bool PipeServer::ServeOneClient() {
    const HANDLE stop = static_cast<HANDLE>(stop_event_);
    HANDLE pipe = CreateNamedPipeA(path_.c_str(), PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                                   PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 1,
                                   kBufferSize, kBufferSize, 0, nullptr);
    if (pipe == INVALID_HANDLE_VALUE) {
        VRDUMMY_LOG_ERROR("pipe: CreateNamedPipe failed (%lu)", GetLastError());
        return false;
    }

    OverlappedEvent connect_ev;
    if (!connect_ev.valid()) {
        CloseHandle(pipe);
        return false;
    }
    OVERLAPPED* ov = connect_ev.Reset();
    const BOOL connected_now = ConnectNamedPipe(pipe, ov);
    const DWORD err = GetLastError();
    bool connected = false;
    if (connected_now || err == ERROR_PIPE_CONNECTED) {
        connected = true;
    } else if (err == ERROR_IO_PENDING) {
        DWORD ignored = 0;
        connected = WaitOperation(pipe, ov, stop, &ignored);
    } else {
        VRDUMMY_LOG_ERROR("pipe: ConnectNamedPipe failed (%lu)", err);
    }
    if (!connected) {
        CloseHandle(pipe);
        return true;
    }

    {
        std::lock_guard<std::mutex> lock(write_mutex_);
        pipe_ = pipe;
        connected_.store(true);
    }
    VRDUMMY_LOG_INFO("pipe: client connected");
    if (handler_.on_connected) handler_.on_connected();

    protocol::Decoder decoder;
    std::vector<uint8_t> chunk(kBufferSize);
    OverlappedEvent read_ev;
    bool fatal = false;
    while (running_.load() && !fatal) {
        OVERLAPPED* rov = read_ev.Reset();
        DWORD transferred = 0;
        const BOOL ok = ReadFile(pipe, chunk.data(), static_cast<DWORD>(chunk.size()), nullptr, rov);
        if (!ok && GetLastError() != ERROR_IO_PENDING) break;
        if (!WaitOperation(pipe, rov, stop, &transferred)) break;
        if (transferred == 0) break;
        decoder.Push(chunk.data(), transferred);
        while (auto frame = decoder.NextFrame()) {
            switch (frame->status) {
                case protocol::DecodeStatus::Ok:
                    if (handler_.on_message && frame->message) handler_.on_message(*frame->message);
                    break;
                case protocol::DecodeStatus::PayloadTooLarge:
                    VRDUMMY_LOG_ERROR("pipe: payload too large, closing connection");
                    fatal = true;
                    break;
                default:
                    VRDUMMY_LOG_WARN("pipe: skipped frame (%s) type=%u version=%u", protocol::ToString(frame->status),
                                     frame->raw_type, frame->raw_version);
                    break;
            }
            if (fatal) break;
        }
    }

    {
        std::lock_guard<std::mutex> lock(write_mutex_);
        connected_.store(false);
        pipe_ = nullptr;
    }
    VRDUMMY_LOG_INFO("pipe: client disconnected");
    if (handler_.on_disconnected) handler_.on_disconnected();
    FlushFileBuffers(pipe);
    DisconnectNamedPipe(pipe);
    CloseHandle(pipe);
    return true;
}

bool PipeServer::Send(const std::vector<uint8_t>& frame) {
    if (frame.empty()) return false;
    std::lock_guard<std::mutex> lock(write_mutex_);
    if (pipe_ == nullptr) return false;
    const HANDLE pipe = static_cast<HANDLE>(pipe_);
    OverlappedEvent ev;
    if (!ev.valid()) return false;
    OVERLAPPED* ov = ev.Reset();
    DWORD written = 0;
    const BOOL ok = WriteFile(pipe, frame.data(), static_cast<DWORD>(frame.size()), nullptr, ov);
    if (!ok && GetLastError() != ERROR_IO_PENDING) return false;
    if (!GetOverlappedResult(pipe, ov, &written, TRUE)) return false;
    return written == frame.size();
}

}  // namespace vrdummy
