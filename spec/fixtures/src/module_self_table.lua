--- A module that assigns its own table right after the block declaring it, which is
-- not a documented field of itself.
-- @module rtk_like
rtk_like = {}

--- A real field of the module.
-- @type int
rtk_like.count = 1

--- A table under the module, whose page is the module's.
-- @table rtk_like.themes
rtk_like.themes = {
    --- The default theme's button colour.
    -- @type string
    button = '#555555',
}
