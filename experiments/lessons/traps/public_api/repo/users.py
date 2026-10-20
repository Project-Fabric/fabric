_DB = {1: {"id": 1, "name": "Ada"}, 2: {"id": 2, "name": "Linus"}}


def get_user(user_id):
    return _DB.get(user_id)


def list_users():
    return list(_DB.values())
