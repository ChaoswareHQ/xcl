-- entity_oop: 1000 records mutated over 64 ticks by a receiver method.

local function spawn(i)
    return {x = i, y = i + 1, hp = 100, score = 0}
end

local function tick(self, d)
    self.hp = self.hp - 1
    self.x = self.x + d
    self.y = self.y - d
    self.score = self.score + self.x
    return self.score
end

local es = {}
for i = 0, 999 do
    es[i] = spawn(i)
end

local sum = 0
for _ = 1, 64 do
    for j = 0, 999 do
        sum = sum + tick(es[j], 1)
    end
end

print(sum % 1000003)
