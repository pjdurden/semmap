class RpcClient:
    def __init__(self, url):
        self.url = url

    def call(self, method):
        return method


def connect(url):
    return RpcClient(url)
