local function hanoi(n, frm, to, via)
    if n == 0 then
        return 0
    end
    local x = hanoi(n - 1, frm, via, to)
    local y = hanoi(n - 1, via, to, frm)
    return x + 1 + y
end
print(hanoi(20, 1, 3, 2))
