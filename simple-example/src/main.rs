use halo2_proofs::{circuit::Value, dev::MockProver, pasta::Fp};
use simple_example::MyCircuit;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let k = 4;

    let constant = Fp::from(7);
    let a = Fp::from(2);
    let b = Fp::from(3);
    let c = constant * a.square() * b.square();

    let circuit = MyCircuit::new(constant, Value::known(a), Value::known(b));

    let mut public_inputs = vec![c];
    let prover = MockProver::run(k, &circuit, vec![public_inputs.clone()])?;
    assert_eq!(prover.verify(), Ok(()));

    // negative case
    public_inputs[0] += Fp::one();
    let prover = MockProver::run(k, &circuit, vec![public_inputs])?;
    assert!(prover.verify().is_err());

    Ok(())
}
