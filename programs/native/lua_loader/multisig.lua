-- M-N Multisig. Pass in a table "{m=M, n=N, tokens=T}" where M is the number
-- of signatures required, and N is a list of the pubkeys identifying
-- those signatures. Once M of len(N) signatures are collected, tokens T
-- are subtracted from account 1 and given to account 4. Note that unlike
-- Rust, Lua is one-based and that account 1 is the first account.

function find(t, x)
    for i, v in pairs(t) do
        if v == x then
            return i
        end
    end
end

function deserialize(bytes)
    return load("return" .. bytes)()
end

local from_account,
      serialize_account,
      state_account,
      to_account = table.unpack(accounts)

local serialize = load(serialize_account.userdata)().serialize

if #state_account.userdata == 0 then
    local cfg = deserialize(data)
    state_account.userdata = serialize(cfg, nil, "s")
    return
end

local cfg = deserialize(state_account.userdata)
local key = deserialize(data)

local i = find(cfg.n, key)
if i == nil then
    return
end

table.remove(cfg.n, i)
cfg.m = cfg.m - 1
state_account.userdata = serialize(cfg, nil, "s")

if cfg.m == 0 then
    from_account.tokens = from_account.tokens - cfg.tokens
    to_account.tokens = to_account.tokens + cfg.tokens

    -- End of game.
    state_account.tokens = 0
end
