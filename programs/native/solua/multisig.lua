-- M-N Multisig. Pass in a table "{m=M, n=N, tokens=T}" where M is the number
-- of signatures required, and N is a list of the pubkeys identifying
-- those signatures. Once M of len(N) signatures are collected, tokens T
-- are subtracted from account 1 and given to account 4. Note that unlike
-- Rust, Lua is one-based and that account 1 is the first account.

local serialize = load(accounts[2].userdata)().serialize

function find(t, x)
    for i, v in pairs(t) do
        if v == x then
            return i
        end
    end
end

if #accounts[3].userdata == 0 then
    local cfg = load("return" .. data)()
    accounts[3].userdata = serialize(cfg, nil, "s")
    return
end

local cfg = load("return" .. accounts[3].userdata)()
local key = load("return" .. data)()

local i = find(cfg.n, key)
if i == nil then
    return
end

table.remove(cfg.n, i)
cfg.m = cfg.m - 1
accounts[3].userdata = serialize(cfg, nil, "s")

if cfg.m == 0 then
    accounts[1].tokens = accounts[1].tokens - cfg.tokens
    accounts[4].tokens = accounts[4].tokens + cfg.tokens

    -- End of game.
    accounts[1].tokens = 0
    accounts[2].tokens = 0
end
