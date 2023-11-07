use std::marker::PhantomData;

use halo2_proofs::{
    arithmetic::Field,
    circuit::{AssignedCell, Chip, Layouter, SimpleFloorPlanner, Value},
    plonk::{Advice, Circuit, Column, ConstraintSystem, Error, Fixed, Instance, Selector},
    poly::Rotation,
};

trait Instructions<F: Field>: Chip<F> {
    type Num;

    fn load_private(&self, layouter: impl Layouter<F>, value: Value<F>)
        -> Result<Self::Num, Error>;

    fn load_constant(&self, layouter: impl Layouter<F>, constant: F) -> Result<Self::Num, Error>;

    fn mul(
        &self,
        layouter: impl Layouter<F>,
        a: Self::Num,
        b: Self::Num,
    ) -> Result<Self::Num, Error>;

    fn expose_public(
        &self,
        layouter: impl Layouter<F>,
        num: Self::Num,
        row: usize,
    ) -> Result<(), Error>;
}

#[derive(Debug, Clone)]
pub struct FieldConfig {
    advice: [Column<Advice>; 2],
    instance: Column<Instance>,
    s_mul: Selector,
}

struct FieldChip<F> {
    config: FieldConfig,
    _marker: PhantomData<F>,
}

impl<F: Field> Chip<F> for FieldChip<F> {
    type Config = FieldConfig;
    type Loaded = ();

    fn config(&self) -> &Self::Config {
        &self.config
    }

    fn loaded(&self) -> &Self::Loaded {
        &()
    }
}

impl<F: Field> FieldChip<F> {
    fn construct(config: <Self as Chip<F>>::Config) -> Self {
        Self {
            config,
            _marker: PhantomData,
        }
    }

    fn configure(
        meta: &mut ConstraintSystem<F>,
        advice: [Column<Advice>; 2],
        instance: Column<Instance>,
        constant: Column<Fixed>,
    ) -> <Self as Chip<F>>::Config {
        meta.enable_equality(instance);
        meta.enable_equality(constant);
        for column in &advice {
            meta.enable_equality(*column);
        }

        let s_mul = meta.selector();

        // create the multiplication gate
        meta.create_gate("mul", |meta| {
            // a9 | a1 | s_mul
            //----------------
            // lhs | rhs | s_mul
            // out
            let lhs = meta.query_advice(advice[0], Rotation::cur());
            let rhs = meta.query_advice(advice[1], Rotation::cur());
            let out = meta.query_advice(advice[0], Rotation::next());
            let s_mul = meta.query_selector(s_mul);

            // the polynomial is: s_mul * (lhs * rhs - out) == 0
            vec![s_mul * (lhs * rhs - out)]
        });

        // return the configuration

        FieldConfig {
            advice,
            instance,
            s_mul,
        }
    }
}

// implement the instructions for the chip

#[derive(Clone)]
struct Number<F: Field>(AssignedCell<F, F>);

impl<F: Field> Instructions<F> for FieldChip<F> {
    type Num = Number<F>;

    // load a number as private input into the circuit
    fn load_private(
        &self,
        mut layouter: impl Layouter<F>,
        value: Value<F>,
    ) -> Result<Self::Num, Error> {
        let config = self.config();

        layouter.assign_region(
            || "load private",
            |mut region| {
                region
                    .assign_advice(|| "private input", config.advice[0], 0, || value)
                    .map(Number)
            },
        )
    }

    // load a constant as a private input into the circuit
    fn load_constant(
        &self,
        mut layouter: impl Layouter<F>,
        constant: F,
    ) -> Result<Self::Num, Error> {
        let config = self.config();

        layouter.assign_region(
            || "load constant",
            |mut region| {
                region
                    .assign_advice_from_constant(|| "constant", config.advice[0], 0, constant)
                    .map(Number)
            },
        )
    }

    // multiply the values and load into the circuit
    fn mul(
        &self,
        mut layouter: impl Layouter<F>,
        a: Self::Num,
        b: Self::Num,
    ) -> Result<Self::Num, Error> {
        let config = self.config();

        layouter.assign_region(
            || "mul",
            |mut region| {
                // enable the selector in the region at offset 0. This will enable the selector
                // for cells at offsets 0 and 1 in this case.
                config.s_mul.enable(&mut region, 0)?;

                // copy the advice values into the region
                a.0.copy_advice(|| "lhs", &mut region, config.advice[0], 0)?;
                b.0.copy_advice(|| "rhs", &mut region, config.advice[1], 0)?;

                // out
                let value = a.0.value().copied() * b.0.value();
                // assign `out` to advice column 0 at offset 1
                region
                    .assign_advice(|| "lhs * rhs", config.advice[1], 0, || value)
                    .map(Number)
            },
        )
    }

    // load the public input into the circuit
    fn expose_public(
        &self,
        mut layouter: impl Layouter<F>,
        num: Self::Num,
        row: usize,
    ) -> Result<(), Error> {
        let config = self.config();
        // constrain equality
        layouter.constrain_instance(num.0.cell(), config.instance, row)
    }
}

// We specify only the private inputs in the circuit definition
#[derive(Default)]
pub struct MyCircuit<F: Field> {
    constant: F,
    a: Value<F>,
    b: Value<F>,
}

impl<F: Field> MyCircuit<F> {
    pub fn new(constant: F, a: Value<F>, b: Value<F>) -> Self {
        Self { constant, a, b }
    }
}

impl<F: Field> Circuit<F> for MyCircuit<F> {
    type Config = FieldConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self::default()
    }

    fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
        // create the two advice columns used by FieldChip for I/O
        let advice = [meta.advice_column(), meta.advice_column()];
        // create the instance column for the public input
        let instance = meta.instance_column();
        // create a fixed column to load constants
        let constant = meta.fixed_column();

        FieldChip::configure(meta, advice, instance, constant)
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<F>,
    ) -> Result<(), Error> {
        let field_chip = FieldChip::<F>::construct(config);

        // load the private values
        let a = field_chip.load_private(layouter.namespace(|| "load a"), self.a)?;
        let b = field_chip.load_private(layouter.namespace(|| "load b"), self.b)?;

        // load the constant
        let constant =
            field_chip.load_constant(layouter.namespace(|| "load constant"), self.constant)?;

        // perform the multiplication like so:
        // ab = a * b
        // absq = ab * ab
        // c = constant * absq
        let ab = field_chip.mul(layouter.namespace(|| "a * b"), a, b)?;
        let absq = field_chip.mul(layouter.namespace(|| "ab * ab"), ab.clone(), ab)?;
        let c = field_chip.mul(layouter.namespace(|| "constant * absq"), constant, absq)?;

        // expose the result as a public input to the circuit
        field_chip.expose_public(layouter.namespace(|| "expose c"), c, 0)
    }
}
