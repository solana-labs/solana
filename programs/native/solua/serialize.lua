----------------------------------------------------------------
-- serialize.lua
--
-- Exports:
--
--  orderedPairs : deterministically ordered version of pairs()
--
--  serialize : convert Lua value to string in Lua syntax
--
----------------------------------------------------------------


-- orderedPairs: iterate over table elements in deterministic order.  First,
-- array elements are returned, then remaining elements sorted by the key's
-- type and value.

-- compare any two Lua values, establishing a complete ordering
local function ltAny(a,b)
   local ta, tb = type(a), type(b)
   if ta ~= tb then
      return ta < tb
   end
   if ta == "string" or ta == "number" then
      return a < b
   end
   return tostring(a) < tostring(b)
end

local inext = ipairs{}

local function orderedPairs(t)
   local keys = {}
   local keyIndex = 1
   local counting = true

   local function _next(seen, s)
      local v

      if counting then
         -- return next array index
         s, v = inext(t, s)
         if s ~= nil then
            seen[s] = true
            return s,v
         end
         counting = false

         -- construct sorted unseen keys
         for k,v in pairs(t) do
            if not seen[k] then
               table.insert(keys, k)
            end
         end
         table.sort(keys, ltAny)
      end

      -- return next unseen table element
      s = keys[keyIndex]
      if s ~= nil then
         keyIndex = keyIndex + 1
         v = t[s]
      end
      return s, v
   end

   return _next, {}, 0
end


-- avoid 'nan', 'inf', and '-inf'
local numtostring = {
   [tostring(-1/0)] = "-1/0",
   [tostring(1/0)] = "1/0",
   [tostring(0/0)] = "0/0"
}

setmetatable(numtostring, { __index = function (t, k) return k end })

-- serialize:  Serialize a Lua data structure
--
--   x    = value to serialize
--   out  = function to be called repeatedly with strings, or
--          table into which strings should be inserted, or
--          nil => return a string
--   iter = function to iterate over table elements, or
--          "s" to sort elements by key, or
--          nil for default (fastest)
--
-- Notes:
--   * Does not support self-referential data structures.
--   * Does not optimize for repeated sub-expressions.
--   * Does not preserve topology; only values.
--   * Does not handle types other than nil, number, boolean, string, table
--
local function serialize(x, out, iter)
   local visited = {}
   local iter = iter=="s" and orderedPairs or iter or pairs
   assert(type(iter) == "function")

   local function _serialize(x)
      if type(x) == "string" then

         out(string.format("%q", x))

      elseif type(x) == "number" then

         out(numtostring[tostring(x)])

      elseif type(x) == "boolean" or
             type(x) == "nil" then

         out(tostring(x))

      elseif type(x) == "table" then

         if visited[x] then
            error("serialize: recursive structure")
         end
         visited[x] = true
         local first, nextIndex = true, 1

         out "{"

         for k,v in iter(x) do
            if first then
               first = false
            else
               out ","
            end
            if k == nextIndex then
               nextIndex = nextIndex + 1
            else
               if type(k) == "string" and k:match("^[%a_][%w_]*$") then
                  out(k.."=")
               else
                  out "["
                  _serialize(k)
                  out "]="
               end
            end
            _serialize(v)
         end

         out "}"
         visited[x] = false
      else
         error("serialize: unsupported type")
      end
   end

   local result
   if not out then
      result = {}
      out = result
   end

   if type(out) == "table" then
      local t = out
      function out(s)
         table.insert(t,s)
      end
   end

   _serialize(x)

   if result then
      return table.concat(result)
   end
end

return {
   orderedPairs = orderedPairs,
   serialize = serialize
}
