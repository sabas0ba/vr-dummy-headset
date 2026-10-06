cmake_minimum_required(VERSION 3.21)

project(vrdummy_driver LANGUAGES CXX)

option(VRDUMMY_BUILD_TESTS "Build unit tests" ON)
option(VRDUMMY_WARNINGS_AS_ERRORS "Treat compiler warnings as errors" ON)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

set(VRDUMMY_REPO_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/..")
set(VRDUMMY_OPENVR_DIR "${VRDUMMY_REPO_ROOT}/third_party/openvr")
set(VRDUMMY_DOCTEST_DIR "${VRDUMMY_REPO_ROOT}/third_party/doctest")
set(VRDUMMY_FIXTURE_DIR "${VRDUMMY_REPO_ROOT}/tests/fixtures")

if(NOT EXISTS "${VRDUMMY_OPENVR_DIR}/headers/openvr_driver.h")
  message(FATAL_ERROR "OpenVR SDK not found. Run: git submodule update --init")
endif()

# Driver name as seen by SteamVR. The output folder layout is
# <build>/vrdummy/{driver.vrdrivermanifest, resources/, bin/win64/driver_vrdummy.dll}
set(VRDUMMY_DRIVER_NAME "vrdummy")
set(VRDUMMY_OUTPUT_DIR "${CMAKE_BINARY_DIR}/${VRDUMMY_DRIVER_NAME}")

function(vrdummy_set_warnings target)
  if(MSVC)
    target_compile_options(${target} PRIVATE /W4 /permissive- /utf-8 /Zc:__cplusplus)
    if(VRDUMMY_WARNINGS_AS_ERRORS)
      target_compile_options(${target} PRIVATE /WX)
    endif()
  else()
    target_compile_options(${target} PRIVATE -Wall -Wextra -Wpedantic)
    if(VRDUMMY_WARNINGS_AS_ERRORS)
      target_compile_options(${target} PRIVATE -Werror)
    endif()
  endif()
endfunction()

# Wire format: no OpenVR dependency, shared by the driver and the tests.
add_library(vrdummy_protocol STATIC src/protocol.cpp src/protocol.h)
target_include_directories(vrdummy_protocol PUBLIC src)
vrdummy_set_warnings(vrdummy_protocol)

add_library(driver_${VRDUMMY_DRIVER_NAME} SHARED
  src/driver_factory.cpp
  src/device_provider.cpp src/device_provider.h
  src/tracked_device.cpp src/tracked_device.h
  src/hmd_device.cpp src/hmd_device.h
  src/controller_device.cpp src/controller_device.h
  src/tracker_device.cpp src/tracker_device.h
  src/pipe_server.cpp src/pipe_server.h
  src/logging.cpp src/logging.h
  src/pose_convert.h
)
# OpenVR headers are third-party and carry unused-parameter warnings under /W4.
target_include_directories(driver_${VRDUMMY_DRIVER_NAME} PRIVATE src)
target_include_directories(driver_${VRDUMMY_DRIVER_NAME} SYSTEM PRIVATE "${VRDUMMY_OPENVR_DIR}/headers")
target_link_libraries(driver_${VRDUMMY_DRIVER_NAME} PRIVATE vrdummy_protocol)
vrdummy_set_warnings(driver_${VRDUMMY_DRIVER_NAME})
set_target_properties(driver_${VRDUMMY_DRIVER_NAME} PROPERTIES
  PREFIX ""
  RUNTIME_OUTPUT_DIRECTORY "${VRDUMMY_OUTPUT_DIR}/bin/win64"
  RUNTIME_OUTPUT_DIRECTORY_DEBUG "${VRDUMMY_OUTPUT_DIR}/bin/win64"
  RUNTIME_OUTPUT_DIRECTORY_RELEASE "${VRDUMMY_OUTPUT_DIR}/bin/win64"
  RUNTIME_OUTPUT_DIRECTORY_RELWITHDEBINFO "${VRDUMMY_OUTPUT_DIR}/bin/win64"
  LIBRARY_OUTPUT_DIRECTORY "${VRDUMMY_OUTPUT_DIR}/bin/linux64"
)

# Copy the manifest and resources next to the binary so that the output
# folder can be registered with vrpathreg directly.
add_custom_command(TARGET driver_${VRDUMMY_DRIVER_NAME} POST_BUILD
  COMMAND ${CMAKE_COMMAND} -E copy_if_different
    "${CMAKE_CURRENT_SOURCE_DIR}/driver.vrdrivermanifest" "${VRDUMMY_OUTPUT_DIR}/driver.vrdrivermanifest"
  COMMAND ${CMAKE_COMMAND} -E copy_directory
    "${CMAKE_CURRENT_SOURCE_DIR}/resources" "${VRDUMMY_OUTPUT_DIR}/resources"
  COMMENT "Copying driver manifest and resources"
)

if(VRDUMMY_BUILD_TESTS)
  enable_testing()
  add_executable(vrdummy_tests
    tests/test_main.cpp
    tests/test_protocol.cpp
    tests/test_pose_convert.cpp
  )
  target_include_directories(vrdummy_tests PRIVATE src)
  target_include_directories(vrdummy_tests SYSTEM PRIVATE "${VRDUMMY_OPENVR_DIR}/headers" "${VRDUMMY_DOCTEST_DIR}")
  target_link_libraries(vrdummy_tests PRIVATE vrdummy_protocol)
  target_compile_definitions(vrdummy_tests PRIVATE VRDUMMY_FIXTURE_DIR="${VRDUMMY_FIXTURE_DIR}")
  vrdummy_set_warnings(vrdummy_tests)
  add_test(NAME vrdummy_tests COMMAND vrdummy_tests)

  # Loads the built driver DLL with fake OpenVR interfaces (no SteamVR).
  add_executable(vrdummy_host_test tests/host/test_driver_host.cpp tests/host/fake_openvr.h)
  target_include_directories(vrdummy_host_test PRIVATE src tests/host)
  target_include_directories(vrdummy_host_test SYSTEM PRIVATE "${VRDUMMY_OPENVR_DIR}/headers" "${VRDUMMY_DOCTEST_DIR}")
  target_link_libraries(vrdummy_host_test PRIVATE vrdummy_protocol)
  target_compile_definitions(vrdummy_host_test PRIVATE
    VRDUMMY_DRIVER_DLL="$<TARGET_FILE:driver_${VRDUMMY_DRIVER_NAME}>")
  add_dependencies(vrdummy_host_test driver_${VRDUMMY_DRIVER_NAME})
  vrdummy_set_warnings(vrdummy_host_test)
  add_test(NAME vrdummy_host_test COMMAND vrdummy_host_test)
  set_tests_properties(vrdummy_host_test PROPERTIES TIMEOUT 60)
endif()

# OpenVR client probe for integration tests against a running SteamVR.
option(VRDUMMY_BUILD_PROBE "Build the OpenVR client probe" ON)
if(VRDUMMY_BUILD_PROBE AND WIN32)
  add_executable(vrdummy_probe tools/vrdummy_probe.cpp)
  target_include_directories(vrdummy_probe SYSTEM PRIVATE "${VRDUMMY_OPENVR_DIR}/headers")
  target_link_libraries(vrdummy_probe PRIVATE "${VRDUMMY_OPENVR_DIR}/lib/win64/openvr_api.lib")
  vrdummy_set_warnings(vrdummy_probe)
  add_custom_command(TARGET vrdummy_probe POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different
      "${VRDUMMY_OPENVR_DIR}/bin/win64/openvr_api.dll" "$<TARGET_FILE_DIR:vrdummy_probe>/openvr_api.dll"
    COMMENT "Copying openvr_api.dll next to vrdummy_probe"
  )
endif()
