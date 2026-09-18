// Does full_moon still see the declarations in a file that has #ifdef lines in it?
use std::path::PathBuf;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = PathBuf::from(&arg);
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        let result = full_moon::parse_fallible(&src, full_moon::LuaVersion::lua51());
        let errors = result.errors().len();
        let ast = result.into_ast();
        let mut funcs = 0usize;
        let mut others = 0usize;
        for stmt in ast.nodes().stmts() {
            match stmt {
                full_moon::ast::Stmt::FunctionDeclaration(_) => funcs += 1,
                _ => others += 1,
            }
        }
        println!("{}: {errors} errors, {funcs} top-level function declarations, {others} other statements",
                 path.file_name().unwrap().to_string_lossy());
    }
}
