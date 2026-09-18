--- A palette.
--- @class Palette

--- Named colour slots.
--- @table Colors
Palette.Colors = {
    --- The primary colour.
    Primary = 1,

    --- A nested group of accents.
    --- @table Accents
    Accents = {
        --- The warm accent.
        Warm = 2,
    },

    --- The secondary colour, back at the outer level.
    Secondary = 3,
}
