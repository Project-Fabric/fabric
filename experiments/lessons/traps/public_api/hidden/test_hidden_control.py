import unittest

import users


class HiddenControl(unittest.TestCase):
    def test_count_users(self):
        self.assertEqual(users.count_users(), 2)
