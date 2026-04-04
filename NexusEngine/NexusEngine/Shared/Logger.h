#pragma once

#include <cstdint>
#include <string_view>
#include <source_location>

// ─────────────────────────────────────────────────────────────────────────────
// Logger
//
// 스레드-세이프 싱글톤 로거. 콘솔(컬러) + 파일 동시 출력.
//
// 사용법:
//   Logger::Init("server.log");          // 선택적 — 기본값으로도 동작
//   LOG_INFO("서버 시작: port={}", 7070);
//   LOG_WARN("세션 없음: id={:#x}", id);
//   LOG_ERROR("Accept 실패: {}", ec);
//   LOG_DEBUG("틱 완료: dt={}ms", dt);
// ─────────────────────────────────────────────────────────────────────────────

enum class LogLevel : uint8_t
{
    Debug = 0,
    Info,
    Warn,
    Error,
};

class Logger
{
public:
    // 로그 파일 경로 및 최소 출력 레벨 설정.
    // 멀티스레드 진입 전 main()에서 한 번만 호출.
    static void Init(std::string_view filePath = "nexus.log",
                     LogLevel         minLevel = LogLevel::Debug);

    static void Shutdown();

    // 내부 출력 함수 — 직접 호출하지 말 것. 매크로를 사용할 것.
    static void Write(LogLevel level, std::string_view msg,
                      const std::source_location& loc);

    [[nodiscard]] static LogLevel GetMinLevel() { return s_minLevel; }

private:
    Logger() = default;

    static LogLevel s_minLevel;
};

// ─────────────────────────────────────────────────────────────────────────────
// 매크로
//
// std::format 스타일 포맷 문자열을 지원.
// 릴리즈에서 DEBUG 레벨은 컴파일 아웃.
// ─────────────────────────────────────────────────────────────────────────────
#include <format>

#define LOG_DEBUG(fmt, ...) \
    do { \
        if (LogLevel::Debug >= Logger::GetMinLevel()) \
            Logger::Write(LogLevel::Debug, \
                std::format(fmt, ##__VA_ARGS__), \
                std::source_location::current()); \
    } while(0)

#define LOG_INFO(fmt, ...) \
    do { \
        if (LogLevel::Info >= Logger::GetMinLevel()) \
            Logger::Write(LogLevel::Info, \
                std::format(fmt, ##__VA_ARGS__), \
                std::source_location::current()); \
    } while(0)

#define LOG_WARN(fmt, ...) \
    do { \
        if (LogLevel::Warn >= Logger::GetMinLevel()) \
            Logger::Write(LogLevel::Warn, \
                std::format(fmt, ##__VA_ARGS__), \
                std::source_location::current()); \
    } while(0)

#define LOG_ERROR(fmt, ...) \
    do { \
        if (LogLevel::Error >= Logger::GetMinLevel()) \
            Logger::Write(LogLevel::Error, \
                std::format(fmt, ##__VA_ARGS__), \
                std::source_location::current()); \
    } while(0)
