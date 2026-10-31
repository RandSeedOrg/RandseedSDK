cmake_minimum_required(VERSION 3.20)

project(randseed_room_host_sdk VERSION 0.1.0 LANGUAGES CXX)

include(FetchContent)
include(CTest)
find_package(Threads REQUIRED)

set(HTTPLIB_USE_OPENSSL OFF CACHE BOOL "" FORCE)
set(HTTPLIB_USE_ZLIB OFF CACHE BOOL "" FORCE)
set(HTTPLIB_USE_BROTLI OFF CACHE BOOL "" FORCE)

FetchContent_Declare(
    nlohmann_json
    GIT_REPOSITORY https://github.com/nlohmann/json.git
    GIT_TAG v3.11.3
    GIT_SHALLOW TRUE
)
FetchContent_Declare(
    cpp_httplib
    GIT_REPOSITORY https://github.com/yhirose/cpp-httplib.git
    GIT_TAG v0.18.6
    GIT_SHALLOW TRUE
)
FetchContent_MakeAvailable(nlohmann_json cpp_httplib)

add_library(randseed_room_host
    src/protocol.cpp
    src/room_host.cpp
)
add_library(randseed::room_host ALIAS randseed_room_host)

target_compile_features(randseed_room_host PUBLIC cxx_std_20)
target_include_directories(randseed_room_host
    PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
)
target_link_libraries(randseed_room_host
    PUBLIC nlohmann_json::nlohmann_json
    PRIVATE httplib::httplib Threads::Threads
)

if(MSVC)
    target_compile_options(randseed_room_host PRIVATE /W4 /permissive-)
else()
    target_compile_options(randseed_room_host PRIVATE -Wall -Wextra -Wpedantic)
endif()

add_executable(room_counter examples/counter/main.cpp)
target_link_libraries(room_counter PRIVATE randseed::room_host)

if(BUILD_TESTING)
    add_executable(randseed_room_host_tests tests/room_host_tests.cpp)
    target_link_libraries(randseed_room_host_tests
        PRIVATE randseed::room_host httplib::httplib Threads::Threads
    )
    add_test(
        NAME randseed_room_host_contract
        COMMAND randseed_room_host_tests
            "${CMAKE_CURRENT_SOURCE_DIR}/tests/../../protocol/fixtures/room-host-v1.json"
    )
endif()
