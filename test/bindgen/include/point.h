#ifndef POINT_H
#define POINT_H

#include <stdint.h>

#define POINT_MAX_COORD 4096

typedef struct Point {
    int32_t x;
    int32_t y;
} Point;

typedef enum Direction {
    DIR_NORTH = 0,
    DIR_EAST = 1,
    DIR_SOUTH = 2,
    DIR_WEST = 3,
} Direction;

#endif
