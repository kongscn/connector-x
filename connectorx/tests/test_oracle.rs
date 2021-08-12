use connectorx::prelude::*;
use connectorx::sources::oracle::{OracleSource, TextProtocol};
use connectorx::sql::CXQuery;
use std::env;

#[test]
fn test_types() {
    let _ = env_logger::builder().is_test(true).try_init();
    let dburl = env::var("ORACLE_URL").unwrap();
    let mut source = OracleSource::<TextProtocol>::new(&dburl, 1).unwrap();
    source.set_queries(&[CXQuery::naked("select test_int from test_table")]);
    source.fetch_metadata().unwrap();
    println!("abccc");
    let mut partitions = source.partition().unwrap();
    assert!(partitions.len() == 1);
    let mut partition = partitions.remove(0);
    partition.prepare().expect("run query");

    assert_eq!(6, partition.nrows());
    assert_eq!(1, partition.ncols());

    let mut parser = partition.parser().unwrap();

    let mut rows:Vec<String> = Vec::new();
    for _i in 0..3 {
        rows.push(parser.produce().unwrap());
    }
    println!("{:?}", rows);
    assert_eq!(rows, vec!["1", "2", "3.3"]);
}