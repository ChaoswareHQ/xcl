# native_call: one builtin call per iteration.
r = 1
i = 0
s = 0
while i < 2000000:
    if isinstance(r, int):
        s = s + 1
    i = i + 1
print(s)
