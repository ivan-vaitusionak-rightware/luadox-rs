// Can full_moon hand back the doc comment that precedes a declaration?
fn main() {
    let src = r#"
--- Documented.
--- @treturn number
function T:count() return 42 end
"#;
    let ast = full_moon::parse_fallible(src, full_moon::LuaVersion::lua51()).into_ast();
    for stmt in ast.nodes().stmts() {
        if let full_moon::ast::Stmt::FunctionDeclaration(f) = stmt {
            let leading: Vec<String> = f
                .function_token()
                .leading_trivia()
                .map(|t| t.to_string().trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            println!("name    = {}", f.name().to_string().trim());
            println!("leading = {leading:?}");
        }
    }
}
