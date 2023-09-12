use std::fmt::Debug;

use crate::{poly_commit::PolyCk, PlonkDomain};
use ark_std::{end_timer, start_timer, One, Zero};
use dist_primitives::utils::domain_utils::EvaluationDomainExt;
use ff::Field;
use ff::{PrimeField, WithSmallOrderMulGroup};
use halo2_proofs::halo2curves::pairing::Engine;
use halo2_proofs::poly::{EvaluationDomain, Rotation};
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq)]
struct ProvingKey<E: Engine> {
    pub ql: Vec<E::Scalar>,
    pub qr: Vec<E::Scalar>,
    pub qm: Vec<E::Scalar>,
    pub qo: Vec<E::Scalar>,
    pub qc: Vec<E::Scalar>,
    pub s1: Vec<E::Scalar>,
    pub s2: Vec<E::Scalar>,
    pub s3: Vec<E::Scalar>,
}

// get the ith element of the domain
pub fn element<F>(i: usize, dom: &EvaluationDomain<F>) -> F
where
    F: PrimeField + WithSmallOrderMulGroup<3> + Serialize + for<'de> Deserialize<'de>,
{
    dom.rotate_omega(F::ONE, Rotation(i as i32))
}

impl<E: Engine> ProvingKey<E> {
    fn new<R: Rng>(n_gates: usize, rng: &mut R) -> Self {
        let outer_time = start_timer!(|| "Dummy CRS");

        let mut qm: Vec<E::Scalar> = vec![E::Scalar::random(&mut *rng); 8 * n_gates];
        let mut ql: Vec<E::Scalar> = qm.clone();
        let mut qr: Vec<E::Scalar> = qm.clone();
        let mut qo: Vec<E::Scalar> = qm.clone();
        let mut qc: Vec<E::Scalar> = qm.clone();
        let mut s1: Vec<E::Scalar> = qm.clone();
        let mut s2: Vec<E::Scalar> = qm.clone();
        let mut s3: Vec<E::Scalar> = qm.clone();

        for i in 0..qm.len() {
            qm[i] = E::Scalar::random(&mut *rng);
            ql[i] = E::Scalar::random(&mut *rng);
            qr[i] = E::Scalar::random(&mut *rng);
            qo[i] = E::Scalar::random(&mut *rng);
            qc[i] = E::Scalar::random(&mut *rng);
            s1[i] = E::Scalar::random(&mut *rng);
            s2[i] = E::Scalar::random(&mut *rng);
            s3[i] = E::Scalar::random(&mut *rng);
        }

        end_timer!(outer_time);

        ProvingKey {
            qm,
            ql,
            qr,
            qo,
            qc,
            s1,
            s2,
            s3,
        }
    }
}

pub fn localplonk<E: Engine + Debug>(pd: &PlonkDomain<E::Scalar>)
where
    <E as Engine>::Scalar:
        PrimeField + WithSmallOrderMulGroup<3> + Serialize + for<'de> Deserialize<'de>,
{
    // Generate CRS ===========================================
    let rng = &mut ark_std::test_rng();
    let pk = ProvingKey::<E>::new(pd.n_gates, &mut *rng);
    let ck: PolyCk<E> = PolyCk::<E>::new(pd.n_gates, &mut *rng);
    let ck8: PolyCk<E> = PolyCk::<E>::new(8 * pd.n_gates, &mut *rng);

    let prover_timer = start_timer!(|| "Prover");
    println!("Round 1===============================");
    // Round 1 ================================================
    // Commit to a, b, c
    let mut aevals = vec![E::Scalar::random(&mut *rng); pd.n_gates];
    let mut bevals = aevals.clone();
    let mut cevals = aevals.clone();
    for i in 0..aevals.len() {
        aevals[i] = E::Scalar::random(&mut *rng);
        bevals[i] = E::Scalar::random(&mut *rng);
        cevals[i] = E::Scalar::random(&mut *rng);
    }

    println!("Committing to a, b, c");
    ck.commit(&aevals);
    println!("aveals: {}", aevals.len());
    ck.commit(&bevals);
    ck.commit(&cevals);
    println!("=======================");

    println!("Extending domain of a,b,c to 8n");
    // do ifft and fft to get evals of a,b,c on the 8n domain
    let mut aevals8 = aevals.clone();
    let mut bevals8 = bevals.clone();
    let mut cevals8 = cevals.clone();

    let fft_timer = start_timer!(|| "FFT");
    PolyCk::<E>::ifft(&mut aevals8, &pd.gates);
    PolyCk::<E>::ifft(&mut bevals8, &pd.gates);
    PolyCk::<E>::ifft(&mut cevals8, &pd.gates);

    PolyCk::<E>::fft(&mut aevals8, &pd.gates8);
    PolyCk::<E>::fft(&mut bevals8, &pd.gates8);
    PolyCk::<E>::fft(&mut cevals8, &pd.gates8);
    end_timer!(fft_timer);

    println!("=======================");

    println!("Round 2===============================");
    // Round 2 ================================================
    // Compute z
    let beta = E::Scalar::random(&mut *rng);
    let gamma = E::Scalar::random(&mut *rng);

    let mut zevals = vec![E::Scalar::ZERO; pd.n_gates];

    let omega = element(1, &pd.gates8);
    let mut omegai = E::Scalar::ONE;

    let pp_timer = start_timer!(|| "PP");
    for i in 0..pd.n_gates {
        // (w_j+σ∗(j)β+γ)(w_{n+j}+σ∗(n+j)β+γ)(w_{2n+j}+σ∗(2n+j)β+γ)
        let den = (aevals[i] + beta * pk.s1[i] + gamma)
            * (bevals[i] + beta * pk.s2[i] + gamma)
            * (cevals[i] + beta * pk.s3[i] + gamma);
        let den = den.invert().unwrap();

        // (w_j+βωj+γ)(w_{n+j}+βk1ωj+γ)(w_{2n+j}+βk2ωj+γ)
        zevals[i] = (aevals[i] + beta * omegai + gamma)
            * (bevals[i] + beta * omegai + gamma)
            * (cevals[i] + beta * omegai + gamma)
            * den;
        omegai *= omega;
    }

    // partial products
    for i in 1..pd.n_gates {
        let last = zevals[i - 1];
        zevals[i] *= last;
    }
    end_timer!(pp_timer);

    // extend to zevals8
    let fft_timer = start_timer!(|| "FFT");
    let mut zevals8 = zevals.clone();
    PolyCk::<E>::ifft(&mut zevals8, &pd.gates);
    PolyCk::<E>::fft(&mut zevals8, &pd.gates8);
    end_timer!(fft_timer);

    println!("Round 3===============================");
    // Round 3 ================================================
    // Compute t
    let alpha = E::Scalar::random(&mut *rng);

    let mut tevals8 = vec![E::Scalar::ZERO; pd.gates8.size()];

    let omega = element(1, &pd.gates8);
    let omegan = element(1, &pd.gates8).pow(&([pd.n_gates as u64]));
    let womegan = (E::Scalar::ZETA * element(1, &pd.gates8)).pow(&([pd.n_gates as u64]));

    let mut omegai = E::Scalar::ONE;
    let mut omegani = E::Scalar::ONE;
    let mut womengani = E::Scalar::ONE;

    let t_timer = start_timer!(|| "Compute t");
    for i in 0..tevals8.len() {
        // ((a(X)b(X)qM(X) + a(X)qL(X) + b(X)qR(X) + c(X)qO(X) + PI(X) + qC(X))
        tevals8[i] += aevals8[i] * bevals8[i] * pk.qm[i]
            + aevals8[i] * pk.ql[i]
            + bevals8[i] * pk.qr[i]
            + cevals8[i] * pk.qo[i]
            + pk.qc[i];

        // ((a(X) + βX + γ)(b(X) + βk1X + γ)(c(X) + βk2X + γ)z(X))*alpha
        tevals8[i] += (aevals8[i] + beta * omegai + gamma)
            * (bevals8[i] + beta * omegai + gamma)
            * (cevals8[i] + beta * omegai + gamma)
            * (omegani - E::Scalar::ONE)
            * alpha;

        // - ((a(X) + βSσ1(X) + γ)(b(X) + βSσ2(X) + γ)(c(X) + βSσ3(X) + γ)z(Xω))*alpha
        tevals8[i] -= (aevals8[i] + beta * pk.s1[i] + gamma)
            * (bevals8[i] + beta * pk.s2[i] + gamma)
            * (cevals8[i] + beta * pk.s3[i] + gamma)
            * (womengani - E::Scalar::ONE)
            * alpha;

        // + (z(X)−1)L1(X)*alpha^2)/Z
        // z(X) is computed using partial products
        tevals8[i] += (zevals8[i]-E::Scalar::ONE)
                        *E::Scalar::ONE //todo:replace with L1
                        *alpha*alpha;

        omegai *= omega;
        omegani *= omegan;
        womengani *= womegan;
    }
    end_timer!(t_timer);

    // divide by ZH
    let fft_timer = start_timer!(|| "FFT");
    PolyCk::<E>::ifft(&mut tevals8, &pd.gates8);
    let mut temp_vec = tevals8[0..7 * pd.n_gates].to_vec();
    PolyCk::<E>::fft(&mut temp_vec, &pd.gates8);
    tevals8[0..7 * pd.n_gates].clone_from_slice(&temp_vec);

    let toep_mat = E::Scalar::from(123 as u64); // packed shares of toeplitz matrix drop from sky
    end_timer!(fft_timer);

    tevals8.iter_mut().for_each(|x| *x *= toep_mat);

    println!("Round 4===============================");
    // Round 4 ================================================
    // commit to z and t
    // open a, b, c, s1, s2, s3, z, t
    // commit and open r = (open_a.open_b)qm + (open_a)ql + (open_b)qr + (open_c)qo + qc

    ck.commit(&zevals);
    ck8.commit(&tevals8);

    let point = E::Scalar::random(&mut *rng);
    let open_a = ck.open(&aevals, point, &pd.gates);
    let open_b = ck.open(&bevals, point, &pd.gates);
    let open_c = ck.open(&cevals, point, &pd.gates);

    // extract every 8th element of pk.s1 using iterators
    ck.open(
        &pk.s1.iter().step_by(8).copied().collect(),
        point,
        &pd.gates,
    );
    ck.open(
        &pk.s2.iter().step_by(8).copied().collect(),
        point,
        &pd.gates,
    );
    ck.open(
        &pk.s3.iter().step_by(8).copied().collect(),
        point,
        &pd.gates,
    );

    let open_ab = open_a * open_b;
    let mut revals = vec![E::Scalar::ZERO; pd.n_gates];
    let timer_r = start_timer!(|| "Compute r");
    for i in 0..pd.n_gates {
        revals[i] = open_ab * pk.qm[i]
            + open_a * pk.ql[i]
            + open_b * pk.qr[i]
            + open_c * pk.qo[i]
            + pk.qc[i];
    }
    end_timer!(timer_r);

    ck.commit(&revals);
    ck.open(&revals, point, &pd.gates);

    end_timer!(prover_timer);
}
