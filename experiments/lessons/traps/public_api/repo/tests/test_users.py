import unittest

import users


class UsersTest(unittest.TestCase):
    def test_seed_data(self):
        self.assertEqual(len(users._DB), 2)
