local x = 42
local i = 0
while i < 2000000 do
    x = (x * 37 + 12345) % 1000003
    i = i + 1
end
print(x)
