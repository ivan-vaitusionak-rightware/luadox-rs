--- A module documented with pipe tables.
-- @module markdown_table
markdown_table = {}

--- Serves one request.
--
-- | Parameter | Required | Description                     |
-- |-----------|----------|---------------------------------|
-- | action    | yes      | Must be set to `config`         |
-- | verbose   | no       | Log every step                  |
--
-- Alignment markers are part of the syntax too:
--
-- | Left | Center | Right |
-- |:-----|:------:|------:|
-- | a    | b      | c     |
--
-- @tparam tab params the request parameters
function markdown_table.serve(params)
end
