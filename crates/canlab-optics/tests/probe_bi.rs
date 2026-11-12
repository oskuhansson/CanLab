use canlab_core::{Program, SymbolId, add, int, mul, pow, sym};
#[test]
fn d() {
    let xv = SymbolId::intern("x");
    let yv = SymbolId::intern("y");
    let e = add(vec![pow(sym("x"), int(2)), pow(sym("y"), int(2)), int(-5)]);
    let p = Program::compile(&e, &[xv, yv]).unwrap();
    let x = (-1.0 - 97f64.sqrt()) / 2.0;
    let y = (-1.0 + 105f64.sqrt()) / 2.0;
    println!("f = {}", p.eval(&[x, y]));
    println!("x^2+y^2 = {}", x * x + y * y);
    let x2 = 0.5f64 * (-1.0 - 97f64.sqrt());
    println!("x2 = {x2} y = {y} f2 = {}", p.eval(&[x2, y]));
}
