cmake_minimum_required(VERSION 3.16)
project(MasmEditor LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Enable Qt automatic tools
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTOUIC ON)
set(CMAKE_AUTORCC ON)

find_package(Qt6 REQUIRED COMPONENTS Core Gui Widgets)

file(GLOB SOURCES "${CMAKE_CURRENT_LIST_DIR}/*.cpp")

add_executable(MasmEditor ${SOURCES})
target_link_libraries(MasmEditor PRIVATE Qt6::Core Qt6::Gui Qt6::Widgets)
