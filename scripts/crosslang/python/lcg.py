x = 42
i = 0
while i < 2000000:
    x = (x * 37 + 12345) % 1000003
    i = i + 1
print(x)
