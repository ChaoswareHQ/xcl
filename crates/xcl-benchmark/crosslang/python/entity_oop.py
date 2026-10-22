# entity_oop: 1000 records mutated over 64 ticks by a receiver method.


def spawn(i):
    return {"x": i, "y": i + 1, "hp": 100, "score": 0}


def tick(entity, d):
    entity["hp"] -= 1
    entity["x"] += d
    entity["y"] -= d
    entity["score"] += entity["x"]
    return entity["score"]


es = [spawn(i) for i in range(1000)]

total = 0
for _ in range(64):
    for entity in es:
        total += tick(entity, 1)

print(total % 1000003)
