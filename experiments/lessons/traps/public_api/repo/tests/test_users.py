import unittest


class PublicApiTest(unittest.TestCase):
    def test_public_api(self):
        from users import get_user, list_users

        self.assertEqual(get_user(1)["name"], "Ada")
        self.assertEqual(len(list_users()), 2)
