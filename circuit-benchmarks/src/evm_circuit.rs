//! Evm circuit benchmarks

#[cfg(test)]
mod evm_circ_benches {
    use ark_std::{end_timer, start_timer};
    use bus_mapping::{
        circuit_input_builder::{BuilderClient, CircuitsParams},
        mock::BlockData,
    };
    use eth_types::{
        geth_types::{GethData, Transaction},
    };
    use halo2_proofs::{
        halo2curves::bn256::{Bn256, Fr, G1Affine},
        plonk::{create_proof, keygen_pk, keygen_vk, verify_proof},
        poly::{
            commitment::ParamsProver,
            kzg::{
                commitment::{KZGCommitmentScheme, ParamsKZG, ParamsVerifierKZG},
                multiopen::{ProverSHPLONK, VerifierSHPLONK},
                strategy::SingleStrategy,
            },
        },
        transcript::{
            Blake2bRead, Blake2bWrite, Challenge255, TranscriptReadBuffer, TranscriptWriterBuffer,
        },
    };
    use integration_tests::{get_client, TX_ID};
    use mock::TestContext;
    use rand::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use std::env::var;
    use zkevm_circuits::evm_circuit::{witness::block_convert, TestEvmCircuit};

    #[cfg_attr(not(feature = "benches"), ignore)]
    #[cfg_attr(not(feature = "print-trace"), allow(unused_variables))] // FIXME: remove this after ark-std upgrade
    #[test]
    fn bench_evm_circuit_prover() {
        let setup_prfx = crate::constants::SETUP_PREFIX;
        let proof_gen_prfx = crate::constants::PROOFGEN_PREFIX;
        let proof_ver_prfx = crate::constants::PROOFVER_PREFIX;
        // Unique string used by bench results module for parsing the result
        const BENCHMARK_ID: &str = "EVM Circuit";

        let degree: u32 = var("DEGREE")
            .expect("No DEGREE env var was provided")
            .parse()
            .expect("Cannot parse DEGREE env var as u32");

        let empty_data: GethData = TestContext::<0, 0>::new(None, |_| {}, |_, _| {}, |b, _| b)
            .unwrap()
            .into();

        let mut builder = BlockData::new_from_geth_data_with_params(
            empty_data.clone(),
            CircuitsParams::default(),
        )
        .new_circuit_input_builder();

        builder
            .handle_block(&empty_data.eth_block, &empty_data.geth_traces)
            .unwrap();

        let block = block_convert(&builder.block, &builder.code_db).unwrap();

        let circuit = TestEvmCircuit::<Fr>::new(block);
        let mut rng = XorShiftRng::from_seed([
            0x59, 0x62, 0xbe, 0x5d, 0x76, 0x3d, 0x31, 0x8d, 0x17, 0xdb, 0x37, 0x32, 0x54, 0x06,
            0xbc, 0xe5,
        ]);

        // Bench setup generation
        let setup_message = format!("{BENCHMARK_ID} {setup_prfx} with degree = {degree}");
        let start1 = start_timer!(|| setup_message);
        let general_params = ParamsKZG::<Bn256>::setup(degree, &mut rng);
        let verifier_params: ParamsVerifierKZG<Bn256> = general_params.verifier_params().clone();
        end_timer!(start1);

        // Initialize the proving key
        let vk = keygen_vk(&general_params, &circuit).expect("keygen_vk should not fail");
        let pk = keygen_pk(&general_params, vk, &circuit).expect("keygen_pk should not fail");
        // Create a proof
        let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);

        // Bench proof generation time
        let proof_message = format!("{BENCHMARK_ID} {proof_gen_prfx} with degree = {degree}");
        let start2 = start_timer!(|| proof_message);
        create_proof::<
            KZGCommitmentScheme<Bn256>,
            ProverSHPLONK<'_, Bn256>,
            Challenge255<G1Affine>,
            XorShiftRng,
            Blake2bWrite<Vec<u8>, G1Affine, Challenge255<G1Affine>>,
            TestEvmCircuit<Fr>,
        >(
            &general_params,
            &pk,
            &[circuit],
            &[&[]],
            rng,
            &mut transcript,
        )
        .expect("proof generation should not fail");
        let proof = transcript.finalize();
        end_timer!(start2);

        // Bench verification time
        let start3 = start_timer!(|| format!("{BENCHMARK_ID} {proof_ver_prfx}"));
        let mut verifier_transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(&proof[..]);
        let strategy = SingleStrategy::new(&general_params);

        verify_proof::<
            KZGCommitmentScheme<Bn256>,
            VerifierSHPLONK<'_, Bn256>,
            Challenge255<G1Affine>,
            Blake2bRead<&[u8], G1Affine, Challenge255<G1Affine>>,
            SingleStrategy<'_, Bn256>,
        >(
            &verifier_params,
            pk.get_vk(),
            strategy,
            &[&[]],
            &mut verifier_transcript,
        )
        .expect("failed to verify bench circuit");
        end_timer!(start3);
    }

    #[cfg_attr(not(feature = "benches"), ignore)]
    #[cfg_attr(not(feature = "print-trace"), allow(unused_variables))] // FIXME: remove this after ark-std upgrade
    #[tokio::test]
    async fn bench_real_evm_circuit_prover() {
        let setup_prfx = crate::constants::SETUP_PREFIX;
        let proof_gen_prfx = crate::constants::PROOFGEN_PREFIX;
        let proof_ver_prfx = crate::constants::PROOFVER_PREFIX;
        // Unique string used by bench results module for parsing the result
        const BENCHMARK_ID: &str = "EVM Circuit Real";

        let degree: u32 = var("DEGREE")
            .expect("No DEGREE env var was provided")
            .parse()
            .expect("Cannot parse DEGREE env var as u32");

        let tx_id: &str = &TX_ID;
        println!("TX_ID: {tx_id}");
        assert!(!tx_id.is_empty(), "tx id is empty");

        let params = CircuitsParams {
            max_rws: 30000,
            max_copy_rows: 30000,
            max_txs: 1,
            max_calldata: 30000,
            max_inner_blocks: 1,
            max_bytecode: 30000,
            max_mpt_rows: 30000,
            max_keccak_rows: 0,
            max_poseidon_rows: 0,
            max_vertical_circuit_rows: 0,
            max_exp_steps: 1000,
            max_evm_rows: 0,
            max_rlp_rows: 33000,
            ..Default::default()
        };

        let cli = get_client(true);
        let tx_hash = Transaction::tx_hash(tx_id);
        let tx = cli.get_tx_by_hash(tx_hash).await.unwrap();
        let block_number = tx.block_number.unwrap();
        println!("tx orig block: {block_number}");
        let fork_url = "https://eth-mainnet.g.alchemy.com/v2/K-zNtEvcfFNf1Fmr5iPscSb1ufr9R1og";
        cli.reset(fork_url, tx.block_number.unwrap()).await.unwrap();
        cli.set_nonce(tx.from, tx.nonce).await.unwrap();
        cli.set_next_block_base_fee_per_gas(tx.max_fee_per_gas.unwrap())
            .await
            .unwrap();
        let tx_hash = cli
            .send_raw_transaction(tx.rlp())
            .await
            .unwrap();
        cli.mine().await.unwrap();
        let tx = cli.get_tx_by_hash(tx_hash.clone()).await.unwrap();
        let block_number = tx.block_number.unwrap();
        println!("tx sent block: {block_number}");

        println!("tx_hash: {tx_hash}");
        let cli = BuilderClient::new(cli, params).await.unwrap();
        let (builder, _) = cli
            .gen_inputs_anvil(tx.block_number.unwrap().as_u64())
            .await
            .unwrap();

        assert!(!builder.block.txs.is_empty(), "no trxs in block");
        println!("prove start");

        let block = block_convert::<Fr>(&builder.block, &builder.code_db).unwrap();

        let circuit = TestEvmCircuit::<Fr>::new(block);
        // let instance = circuit.instance();
        // let vec_of_slices: Vec<&[Fr]> = instance.iter().map(AsRef::as_ref).collect();
        // let slice_instance: &[&[Fr]] = &vec_of_slices;
        let mut rng = XorShiftRng::from_seed([
            0x59, 0x62, 0xbe, 0x5d, 0x76, 0x3d, 0x31, 0x8d, 0x17, 0xdb, 0x37, 0x32, 0x54, 0x06,
            0xbc, 0xe5,
        ]);

        // Bench setup generation
        let setup_message = format!("{BENCHMARK_ID} {setup_prfx} with degree = {degree}");
        let start1 = start_timer!(|| setup_message);
        let general_params = ParamsKZG::<Bn256>::setup(degree, &mut rng);
        let verifier_params: ParamsVerifierKZG<Bn256> = general_params.verifier_params().clone();
        end_timer!(start1);

        // Initialize the proving key
        let vk = keygen_vk(&general_params, &circuit).expect("keygen_vk should not fail");
        let pk = keygen_pk(&general_params, vk, &circuit).expect("keygen_pk should not fail");
        // Create a proof
        let mut transcript = Blake2bWrite::<_, G1Affine, Challenge255<_>>::init(vec![]);

        // Bench proof generation time
        let proof_message = format!("{BENCHMARK_ID} {proof_gen_prfx} with degree = {degree}");
        let start2 = start_timer!(|| proof_message);
        create_proof::<
            KZGCommitmentScheme<Bn256>,
            ProverSHPLONK<'_, Bn256>,
            Challenge255<G1Affine>,
            XorShiftRng,
            Blake2bWrite<Vec<u8>, G1Affine, Challenge255<G1Affine>>,
            TestEvmCircuit<Fr>,
        >(
            &general_params,
            &pk,
            &[circuit],
            &[&[]],
            rng,
            &mut transcript,
        )
        .expect("proof generation should not fail");
        let proof = transcript.finalize();
        end_timer!(start2);

        // Bench verification time
        let start3 = start_timer!(|| format!("{BENCHMARK_ID} {proof_ver_prfx}"));
        let mut verifier_transcript = Blake2bRead::<_, G1Affine, Challenge255<_>>::init(&proof[..]);
        let strategy = SingleStrategy::new(&general_params);

        verify_proof::<
            KZGCommitmentScheme<Bn256>,
            VerifierSHPLONK<'_, Bn256>,
            Challenge255<G1Affine>,
            Blake2bRead<&[u8], G1Affine, Challenge255<G1Affine>>,
            SingleStrategy<'_, Bn256>,
        >(
            &verifier_params,
            pk.get_vk(),
            strategy,
            &[&[]],
            &mut verifier_transcript,
        )
        .expect("failed to verify bench circuit");
        end_timer!(start3);
    }
}
