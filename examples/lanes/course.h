#pragma once

#include <array>
#include <cstdint>
#include <string>

constexpr unsigned course_length = 16;
using Course = std::array<std::uint32_t, course_length>;

std::string generate_course(Course& rows, std::uint32_t seed, const wchar_t* adapter);
