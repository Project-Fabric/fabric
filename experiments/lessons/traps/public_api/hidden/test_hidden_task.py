import unittest

import users


class HiddenTask(unittest.TestCase):
    def test_find_user_by_name(self):
        self.assertEqual(users.find_user_by_name("Linus")["id"], 2)
        self.assertIsNone(users.find_user_by_name("Nobody"))
