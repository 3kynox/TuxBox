import unittest

from tools.check_lanes_input import (ACTIVE, BOTTOM, HEIGHT, IDLE, INK, LEFT, MID, NEXT_ROW, PAPER, RED,
                                     RIGHT, TEAL, TOP, WIDTH, Board, center, frame_pixels, read_board,
                                     step_toward)


def paint(lane=1, hazard: int | None = 2, title=None, player=True):
    """A board with the features the check samples, laid out as examples/lanes/screen.cpp draws them."""
    canvas = [[INK] * WIDTH for _ in range(HEIGHT)]

    def rect(x0, y0, x1, y1, color):
        for y in range(y0, y1):
            canvas[y][x0:x1] = [color] * (x1 - x0)
    for i in range(3):
        rect(LEFT + i * (RIGHT - LEFT) // 3 + 1, TOP, LEFT + (i + 1) * (RIGHT - LEFT) // 3 - 1, BOTTOM,
             ACTIVE if i == lane else IDLE)
    if hazard is not None:
        for row in range(29):
            radius = row * 16 // 28
            rect(center(hazard) - radius, NEXT_ROW - 16 + row, center(hazard) + radius + 1, NEXT_ROW - 15 + row, RED)
    if player:
        rect(center(lane) - 10, BOTTOM - 37, center(lane) + 11, BOTTOM - 10, TEAL)
    if title:
        rect(LEFT, MID - 53, RIGHT, MID + 53, INK)
        width = (len(title) * 6 - 1) * 3
        x0 = (WIDTH - width) // 2
        rect(x0, MID - 31, x0 + 3, MID - 10, PAPER)  # first and last lit columns of the title
        rect(x0 + width - 3, MID - 31, x0 + width, MID - 10, PAPER)
    return lambda x, y: canvas[y][x]


class BoardTests(unittest.TestCase):
    def test_reads_lane_and_next_barrier(self):
        for lane in range(3):
            for hazard in range(3):
                self.assertEqual(read_board(paint(lane, hazard)), Board(lane, "playing", hazard))

    def test_reads_the_finish_banners(self):
        self.assertEqual(read_board(paint(2, 1, "COLLISION")).finish, "crashed")
        self.assertEqual(read_board(paint(0, None, "COURSE COMPLETE")), Board(0, "won", None))

    def test_rejects_frames_that_are_not_the_board(self):
        black = lambda _x, _y: (0, 0, 0)
        with self.assertRaisesRegex(ValueError, "no Lanes board"):
            read_board(black)
        with self.assertRaisesRegex(ValueError, "no player"):
            read_board(paint(player=False))
        with self.assertRaisesRegex(ValueError, "unrecognized finish banner"):
            read_board(paint(title="PAUSED"))

    def test_steering_reaches_any_barrier_within_two_moves(self):
        for lane in range(3):
            for hazard in range(3):
                moves, position = 0, lane
                while True:
                    step = step_toward(position, hazard)
                    reach = abs(hazard - position) <= 1
                    position += step
                    moves += 1
                    if reach:
                        break
                    self.assertNotEqual(position, hazard, "a safe step landed on the barrier")
                self.assertEqual(position, hazard)
                self.assertLessEqual(moves, 2)


class FrameTests(unittest.TestCase):
    def test_decodes_32_bit_zpixmap_with_padding(self):
        stride = 12  # two pixels and four bytes of row padding
        rows = [bytes([0x11, 0x22, 0x33, 0, 0x44, 0x55, 0x66, 0]) + b"\xee" * 4,
                bytes([0x77, 0x88, 0x99, 0, 0xaa, 0xbb, 0xcc, 0]) + b"\xee" * 4]
        pixel = frame_pixels(b"".join(rows), 2, 2, stride, (0xff0000, 0xff00, 0xff))
        self.assertEqual(pixel(0, 0), (0x33, 0x22, 0x11))
        self.assertEqual(pixel(1, 1), (0xcc, 0xbb, 0xaa))
        with self.assertRaisesRegex(ValueError, "outside"):
            pixel(2, 0)


if __name__ == "__main__":
    unittest.main()
