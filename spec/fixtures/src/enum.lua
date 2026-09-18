--- Focus scope kinds.
--- @enum FocusScope
FocusScope = {
    --- Focus is trapped inside the scope.
    Trap = 0,

    Undocumented = 1,

    --- A hexadecimal value.
    Hex = 0x10,

    --- A value that is not an integer literal.
    Text = "no",
}
