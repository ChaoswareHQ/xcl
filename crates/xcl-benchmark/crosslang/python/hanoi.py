def hanoi(n, frm, to, via):
    if n == 0:
        return 0
    x = hanoi(n - 1, frm, via, to)
    y = hanoi(n - 1, via, to, frm)
    return x + 1 + y

print(hanoi(20, 1, 3, 2))
