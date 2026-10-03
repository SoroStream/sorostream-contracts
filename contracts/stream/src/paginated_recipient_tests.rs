use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, Vec,
};

struct TestEnv {
    env: Env,
    contract_id: Address,
    token_id: Address,
    sender: Address,
    recipient: Address,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let contract_id = env.register(SoroStreamContract, ());
    let token_admin = Address::generate(&env);
    let token_id = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    StellarAssetClient::new(&env, &token_id).mint(&sender, &10_000_000);

    SoroStreamContractClient::new(&env, &contract_id).set_min_duration(&sender, &0u64);

    TestEnv {
        env,
        contract_id,
        token_id,
        sender,
        recipient,
    }
}

fn client(t: &TestEnv) -> SoroStreamContractClient<'_> {
    SoroStreamContractClient::new(&t.env, &t.contract_id)
}

#[test]
fn test_get_streams_by_recipient_pagination() {
    let t = setup();
    let c = client(&t);

    let num_streams = 5;
    for i in 0..num_streams {
        let offset = (i * 1000) as u64;
        c.create_stream(
            &t.sender,
            &t.recipient,
            &t.token_id,
            &100_000,
            &1000,
            &offset,
            &0u64,
            &false,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &None::<u32>,
        );
    }

    let page1 = c.get_streams_by_recipient(&t.recipient, &0u64, &2u32);
    assert_eq!(page1.len(), 2, "First page should have 2 streams");

    let cursor = if page1.len() > 0 {
        page1.last().unwrap().clone()
    } else {
        0u64
    };

    let page2 = c.get_streams_by_recipient(&t.recipient, &cursor, &2u32);
    assert!(page2.len() > 0, "Should have more streams in next page");

    if page2.len() > 0 && page1.len() > 0 {
        assert!(page2[0] != page1[0], "Pages should contain different streams");
    }
}

#[test]
fn test_get_streams_by_recipient_empty_result() {
    let t = setup();
    let c = client(&t);

    let other_recipient = Address::generate(&t.env);
    let result = c.get_streams_by_recipient(&other_recipient, &0u64, &10u32);

    assert_eq!(result.len(), 0, "Should return empty for recipient with no streams");
}

#[test]
fn test_get_streams_by_recipient_ordering() {
    let t = setup();
    let c = client(&t);

    let mut stream_ids = Vec::new(&t.env);
    for i in 0..3 {
        let start_time = ((2 - i) * 100) as u64;
        let id = c.create_stream(
            &t.sender,
            &t.recipient,
            &t.token_id,
            &100_000,
            &1000,
            &start_time,
            &0u64,
            &false,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &None::<u32>,
        );
        stream_ids.push_back(id);
    }

    let retrieved = c.get_streams_by_recipient(&t.recipient, &0u64, &100u32);
    assert_eq!(retrieved.len(), 3, "Should retrieve all 3 streams");

    for i in 0..(retrieved.len() - 1) {
        assert!(retrieved[i] >= retrieved[i + 1], "Should be ordered by creation ledger descending");
    }
}

#[test]
fn test_get_streams_by_recipient_cursor_validation() {
    let t = setup();
    let c = client(&t);

    for _ in 0..5 {
        c.create_stream(
            &t.sender,
            &t.recipient,
            &t.token_id,
            &100_000,
            &1000,
            &0,
            &0u64,
            &false,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &None::<u32>,
        );
    }

    let page1 = c.get_streams_by_recipient(&t.recipient, &0u64, &2u32);
    let cursor = page1.last().unwrap().clone();
    let page2 = c.get_streams_by_recipient(&t.recipient, &cursor, &2u32);

    assert!(page1.len() > 0, "First page should have streams");
    assert!(page2.len() > 0, "Second page should have streams");
    assert!(page1[0] != page2[0], "Different cursors should return different pages");
}

#[test]
fn test_get_streams_by_recipient_page_size_honored() {
    let t = setup();
    let c = client(&t);

    for _ in 0..10 {
        c.create_stream(
            &t.sender,
            &t.recipient,
            &t.token_id,
            &100_000,
            &1000,
            &0,
            &0u64,
            &false,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &None::<u32>,
        );
    }

    let page_size_3 = c.get_streams_by_recipient(&t.recipient, &0u64, &3u32);
    assert!(page_size_3.len() <= 3, "Should not exceed requested page size");

    let page_size_5 = c.get_streams_by_recipient(&t.recipient, &0u64, &5u32);
    assert!(page_size_5.len() <= 5, "Should not exceed requested page size");
}

#[test]
fn test_get_streams_by_recipient_complete_iteration() {
    let t = setup();
    let c = client(&t);

    for _ in 0..7 {
        c.create_stream(
            &t.sender,
            &t.recipient,
            &t.token_id,
            &100_000,
            &1000,
            &0,
            &0u64,
            &false,
            &0u64,
            &false,
            &0i128,
            &None::<u32>,
            &None::<i128>,
            &None::<u32>,
        );
    }

    let page1 = c.get_streams_by_recipient(&t.recipient, &0u64, &3u32);
    let cursor1 = page1.last().unwrap().clone();

    let page2 = c.get_streams_by_recipient(&t.recipient, &cursor1, &3u32);
    let cursor2 = page2.last().unwrap().clone();

    let page3 = c.get_streams_by_recipient(&t.recipient, &cursor2, &3u32);

    let total = page1.len() + page2.len() + page3.len();
    assert_eq!(total, 7, "Pagination should cover all streams");
}
