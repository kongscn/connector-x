//! Transport from MySQL Source to Arrow Destination.

use crate::{
    destinations::arrow::{typesystem::ArrowTypeSystem, ArrowDestination, ArrowDestinationError},
    impl_transport,
    sources::mysql::{
        BinaryProtocol, MySQLSource, MySQLSourceError, MySQLTypeSystem, TextProtocol,
    },
    typesystem::TypeConversion,
};
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use num_traits::ToPrimitive;
use rust_decimal::Decimal;
use std::marker::PhantomData;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MySQLArrowTransportError {
    #[error(transparent)]
    Source(#[from] MySQLSourceError),

    #[error(transparent)]
    Destination(#[from] ArrowDestinationError),

    #[error(transparent)]
    ConnectorX(#[from] crate::errors::ConnectorXError),
}

/// Convert MySQL data types to Arrow data types.
pub struct MySQLArrowTransport<P>(PhantomData<P>);

impl_transport!(
    name = MySQLArrowTransport<BinaryProtocol>,
    error = MySQLArrowTransportError,
    systems = MySQLTypeSystem => ArrowTypeSystem,
    route = MySQLSource<BinaryProtocol> => ArrowDestination,
    mappings = {
        { Double[f64]                => Float64[f64]            | conversion auto }
        { Long[i32]                  => Int64[i64]              | conversion auto }
        { LongLong[i64]              => Int64[i64]              | conversion auto }
        { Date[NaiveDate]            => Date32[NaiveDate]       | conversion auto }
        { Time[NaiveTime]            => Time64[NaiveTime]       | conversion auto }
        { Datetime[NaiveDateTime]    => Date64[NaiveDateTime]   | conversion auto }
        { Decimal[Decimal]           => Float64[f64]            | conversion option }
        { VarChar[String]            => LargeUtf8[String]       | conversion auto }
        { Char[String]               => LargeUtf8[String]       | conversion none }
    }
);

impl_transport!(
    name = MySQLArrowTransport<TextProtocol>,
    error = MySQLArrowTransportError,
    systems = MySQLTypeSystem => ArrowTypeSystem,
    route = MySQLSource<TextProtocol> => ArrowDestination,
    mappings = {
        { Double[f64]                => Float64[f64]            | conversion auto }
        { Long[i32]                  => Int64[i64]              | conversion auto }
        { LongLong[i64]              => Int64[i64]              | conversion auto }
        { Date[NaiveDate]            => Date32[NaiveDate]       | conversion auto }
        { Time[NaiveTime]            => Time64[NaiveTime]       | conversion auto }
        { Datetime[NaiveDateTime]    => Date64[NaiveDateTime]   | conversion auto }
        { Decimal[Decimal]           => Float64[f64]            | conversion option }
        { VarChar[String]            => LargeUtf8[String]       | conversion auto }
        { Char[String]               => LargeUtf8[String]       | conversion none }
    }
);

impl<P> TypeConversion<Decimal, f64> for MySQLArrowTransport<P> {
    fn convert(val: Decimal) -> f64 {
        val.to_f64()
            .unwrap_or_else(|| panic!("cannot convert decimal {:?} to float64", val))
    }
}
