m = {"a": 1, "b": 2, "c": 3, "d": 4, "e": 5, "f": 6, "g": 7, "h": 8}
i = 0
s = 0
while i < 500000:
    s = s + m["a"]
    s = s + m["b"]
    s = s + m["c"]
    s = s + m["d"]
    s = s + m["e"]
    s = s + m["f"]
    s = s + m["g"]
    s = s + m["h"]
    i = i + 1
print(s)
