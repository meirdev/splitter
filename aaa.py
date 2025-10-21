import time
import random

for i in range(100000):
    print(i, flush=True)

    if random.randint(1, 10) == 8:
        time.sleep(5)
