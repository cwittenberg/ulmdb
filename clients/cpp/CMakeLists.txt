cmake_minimum_required(VERSION 3.16)
project(ulmdb_cpp VERSION 0.1.0 LANGUAGES CXX)

find_package(Threads REQUIRED)
find_library(ULMDB_C_LIBRARY NAMES ulmdb_c
    HINTS "${CMAKE_CURRENT_SOURCE_DIR}/../../target/debug"
          "${CMAKE_CURRENT_SOURCE_DIR}/../../target/release"
    DOC "ULMDB C ABI shared or static library")
if(NOT ULMDB_C_LIBRARY)
    message(FATAL_ERROR "Build the C ABI with cargo build -p ulmdb-c, or set ULMDB_C_LIBRARY")
endif()

add_library(ulmdb_cpp INTERFACE)
add_library(ulmdb::cpp ALIAS ulmdb_cpp)
target_compile_features(ulmdb_cpp INTERFACE cxx_std_17)
target_include_directories(ulmdb_cpp INTERFACE
    "${CMAKE_CURRENT_SOURCE_DIR}/include"
    "${CMAKE_CURRENT_SOURCE_DIR}/../c/include")
target_link_libraries(ulmdb_cpp INTERFACE "${ULMDB_C_LIBRARY}" Threads::Threads)
if(CMAKE_SYSTEM_NAME STREQUAL "Linux" AND ULMDB_C_LIBRARY MATCHES "\\.a$")
    target_link_libraries(ulmdb_cpp INTERFACE ${CMAKE_DL_LIBS} m rt util)
endif()

option(ULMDB_CPP_BUILD_TESTS "Build the C++ client tests" ON)
option(ULMDB_CPP_BUILD_EXAMPLES "Build the C++ client examples" ON)
function(ulmdb_cpp_warnings target)
    if(CMAKE_CXX_COMPILER_ID MATCHES "GNU|Clang")
        target_compile_options(${target} PRIVATE -Wall -Wextra -Wpedantic -Werror)
    endif()
endfunction()
if(ULMDB_CPP_BUILD_TESTS)
    enable_testing()
    add_executable(ulmdb_cpp_tests tests/client.cpp tests/lifetimes.cpp tests/routes.cpp)
    target_link_libraries(ulmdb_cpp_tests PRIVATE ulmdb::cpp)
    ulmdb_cpp_warnings(ulmdb_cpp_tests)
    add_test(NAME ulmdb_cpp_client COMMAND ulmdb_cpp_tests)
endif()
if(ULMDB_CPP_BUILD_EXAMPLES)
    foreach(example basic routes)
        add_executable(ulmdb_cpp_${example} examples/${example}.cpp)
        target_link_libraries(ulmdb_cpp_${example} PRIVATE ulmdb::cpp)
        ulmdb_cpp_warnings(ulmdb_cpp_${example})
    endforeach()
endif()
