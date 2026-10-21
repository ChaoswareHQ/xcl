local bit = require("bit")
local x = 12345
local i = 0
while i < 2000000 do
    x = bit.band(bit.bor(bit.lshift(x, 5), bit.rshift(x, 3)), 0xFFFFFF)
    x = bit.band(x + i, 0xFFFFFF)
    i = i + 1
end
print(x)
