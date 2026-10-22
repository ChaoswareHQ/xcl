-- native_call: one builtin call per iteration.
local r = 1
local i = 0
local s = 0
while i < 2000000 do
    if type(r) == "number" then
        s = s + 1
    end
    i = i + 1
end
print(s)
