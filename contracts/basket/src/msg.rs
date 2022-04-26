use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use cosmwasm_std::{Addr, Uint128, Decimal};
use cw20::{Cw20Coin, MinterResponse, Cw20ReceiveMsg};
use crate::asset::{Asset, AssetInfo};
use crate::state::BasketAsset;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateMsg {
    
    /// The list of assets in the basket
    pub assets: Vec<InstantiateAssetInfo>,
    /// Name of Basket
	pub name: String,
	/// fee for non-stable asset perp
	pub tax_basis_points: Uint128,
	/// fee for stable asset perp
	pub stable_tax_basis_points: Uint128,
	/// base fee for mint/burning lp token
	pub mint_burn_basis_points: Uint128,
	/// base fee for swap
	pub swap_fee_basis_points: Uint128,
	/// base fee for swaping between stable assets 
	pub stable_swap_fee_basis_points: Uint128, 
	/// references position fees, not for funding rate, nor for getting in/out of a position
	pub margin_fee_basis_points: Uint128, 
	/// fee for getting liquidated, goes to liquidator in USD
	pub liquidation_fee_usd: Uint128,
	/// prevents gaming of oracle with hourly trades
	pub min_profit_time: Uint128,
	/// account that can make changes to the exchange
	pub admin: Addr,
    /// The token contract code ID used for the tokens in the pool
    pub token_code_id: u64,

}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecuteMsg {
    DepositLiquidity {
        assets: Vec<Asset>,
        slippage_tolerance: Option<Decimal>,
        receiver: Option<String>,
    },
    Receive { msg: Cw20ReceiveMsg },
    Swap {
        sender: Addr,
        offer_asset: Asset,
        belief_price: Option<Decimal>,
        max_spread: Option<Decimal>,
        to: Option<Addr>,
        ask_asset: AssetInfo,
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QueryMsg {
    // Basket returns the basket as a json-encoded string
    Basket {},
}

// We define a custom struct for each query response
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct CountResponse {
    pub count: u8,
}

#[derive(PartialEq,Clone,Default)]
pub struct MsgInstantiateContractResponse {
    // message fields
    pub contract_address: String,
    pub data: Vec<u8>,
    // special fields for the the Message implementation
    pub unknown_fields: ::protobuf::UnknownFields,
    pub cached_size: ::protobuf::CachedSize,
}

impl MsgInstantiateContractResponse {
    pub fn new() -> MsgInstantiateContractResponse {
        ::std::default::Default::default()
    }

    pub fn get_contract_address(&self) -> &str {
        &self.contract_address
    }
}


impl ::protobuf::Clear for MsgInstantiateContractResponse {
    fn clear(&mut self) {
        self.contract_address.clear();
        self.data.clear();
    }
}

impl ::std::fmt::Debug for MsgInstantiateContractResponse {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        ::protobuf::text_format::fmt(self, f)
    }
}

impl ::protobuf::reflect::ProtobufValue for MsgInstantiateContractResponse {
    fn as_ref(&self) -> ::protobuf::reflect::ReflectValueRef {
        ::protobuf::reflect::ReflectValueRef::Message(self)
    }
}

impl<'a> ::std::default::Default for &'a MsgInstantiateContractResponse {
    fn default() -> &'a MsgInstantiateContractResponse {
        <MsgInstantiateContractResponse as ::protobuf::Message>::default_instance()
    }
}

impl ::protobuf::Message for MsgInstantiateContractResponse {
    fn is_initialized(&self) -> bool {
        true
    }

    fn merge_from(&mut self, is: &mut ::protobuf::CodedInputStream<'_>) -> ::protobuf::ProtobufResult<()> {
        while !is.eof()? {
            let (field_number, wire_type) = is.read_tag_unpack()?;
            match field_number {
                1 => {
                    ::protobuf::rt::read_singular_proto3_string_into(wire_type, is, &mut self.contract_address)?;
                },
                2 => {
                    ::protobuf::rt::read_singular_proto3_bytes_into(wire_type, is, &mut self.data)?;
                },
                _ => {
                    ::protobuf::rt::read_unknown_or_skip_group(field_number, wire_type, is, self.mut_unknown_fields())?;
                },
            };
        }
        ::std::result::Result::Ok(())
    }

    // Compute sizes of nested messages
    #[allow(unused_variables)]
    fn compute_size(&self) -> u32 {
        let mut my_size = 0;
        if !self.contract_address.is_empty() {
            my_size += ::protobuf::rt::string_size(1, &self.contract_address);
        }
        if !self.data.is_empty() {
            my_size += ::protobuf::rt::bytes_size(2, &self.data);
        }
        my_size += ::protobuf::rt::unknown_fields_size(self.get_unknown_fields());
        self.cached_size.set(my_size);
        my_size
    }

    fn write_to_with_cached_sizes(&self, os: &mut ::protobuf::CodedOutputStream<'_>) -> ::protobuf::ProtobufResult<()> {
        if !self.contract_address.is_empty() {
            os.write_string(1, &self.contract_address)?;
        }
        if !self.data.is_empty() {
            os.write_bytes(2, &self.data)?;
        }
        os.write_unknown_fields(self.get_unknown_fields())?;
        ::std::result::Result::Ok(())
    }

    fn get_cached_size(&self) -> u32 {
        self.cached_size.get()
    }

    fn get_unknown_fields(&self) -> &::protobuf::UnknownFields {
        &self.unknown_fields
    }

    fn mut_unknown_fields(&mut self) -> &mut ::protobuf::UnknownFields {
        &mut self.unknown_fields
    }

    fn as_any(&self) -> &dyn (::std::any::Any) {
        self as &dyn (::std::any::Any)
    }
    fn as_any_mut(&mut self) -> &mut dyn (::std::any::Any) {
        self as &mut dyn (::std::any::Any)
    }
    fn into_any(self: ::std::boxed::Box<Self>) -> ::std::boxed::Box<dyn (::std::any::Any)> {
        self
    }

    fn descriptor(&self) -> &'static ::protobuf::reflect::MessageDescriptor {
        Self::descriptor_static()
    }

    fn new() -> MsgInstantiateContractResponse {
        MsgInstantiateContractResponse::new()
    }

    fn descriptor_static() -> &'static ::protobuf::reflect::MessageDescriptor {
        static descriptor: ::protobuf::rt::LazyV2<::protobuf::reflect::MessageDescriptor> = ::protobuf::rt::LazyV2::INIT;
        descriptor.get(|| {
            let mut fields = ::std::vec::Vec::new();
            fields.push(::protobuf::reflect::accessor::make_simple_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                "contract_address",
                |m: &MsgInstantiateContractResponse| { &m.contract_address },
                |m: &mut MsgInstantiateContractResponse| { &mut m.contract_address },
            ));
            fields.push(::protobuf::reflect::accessor::make_simple_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                "data",
                |m: &MsgInstantiateContractResponse| { &m.data },
                |m: &mut MsgInstantiateContractResponse| { &mut m.data },
            ));
            ::protobuf::reflect::MessageDescriptor::new_pb_name::<MsgInstantiateContractResponse>(
                "MsgInstantiateContractResponse",
                fields,
                file_descriptor_proto()
            )
        })
    }

    fn default_instance() -> &'static MsgInstantiateContractResponse {
        static instance: ::protobuf::rt::LazyV2<MsgInstantiateContractResponse> = ::protobuf::rt::LazyV2::INIT;
        instance.get(MsgInstantiateContractResponse::new)
    }
}

static file_descriptor_proto_data: &'static [u8] = b"\
    \n\x12src/response.proto\"_\n\x1eMsgInstantiateContractResponse\x12)\n\
    \x10contract_address\x18\x01\x20\x01(\tR\x0fcontractAddress\x12\x12\n\
    \x04data\x18\x02\x20\x01(\x0cR\x04dataJ\xf8\x02\n\x06\x12\x04\0\0\x08\
    \x01\n\x08\n\x01\x0c\x12\x03\0\0\x12\n_\n\x02\x04\0\x12\x04\x03\0\x08\
    \x01\x1aS\x20MsgInstantiateContractResponse\x20defines\x20the\x20Msg/Ins\
    tantiateContract\x20response\x20type.\n\n\n\n\x03\x04\0\x01\x12\x03\x03\
    \x08&\nR\n\x04\x04\0\x02\0\x12\x03\x05\x02\x1e\x1aE\x20ContractAddress\
    \x20is\x20the\x20bech32\x20address\x20of\x20the\x20new\x20contract\x20in\
    stance.\n\n\x0c\n\x05\x04\0\x02\0\x05\x12\x03\x05\x02\x08\n\x0c\n\x05\
    \x04\0\x02\0\x01\x12\x03\x05\t\x19\n\x0c\n\x05\x04\0\x02\0\x03\x12\x03\
    \x05\x1c\x1d\nO\n\x04\x04\0\x02\x01\x12\x03\x07\x02\x11\x1aB\x20Data\x20\
    contains\x20base64-encoded\x20bytes\x20to\x20returned\x20from\x20the\x20\
    contract\n\n\x0c\n\x05\x04\0\x02\x01\x05\x12\x03\x07\x02\x07\n\x0c\n\x05\
    \x04\0\x02\x01\x01\x12\x03\x07\x08\x0c\n\x0c\n\x05\x04\0\x02\x01\x03\x12\
    \x03\x07\x0f\x10b\x06proto3\
";

static file_descriptor_proto_lazy: ::protobuf::rt::LazyV2<::protobuf::descriptor::FileDescriptorProto> = ::protobuf::rt::LazyV2::INIT;


fn parse_descriptor_proto() -> ::protobuf::descriptor::FileDescriptorProto {
    ::protobuf::Message::parse_from_bytes(file_descriptor_proto_data).unwrap()
}

pub fn file_descriptor_proto() -> &'static ::protobuf::descriptor::FileDescriptorProto {
    file_descriptor_proto_lazy.get(|| {
        parse_descriptor_proto()
    })
}

/// This structure describes the parameters used for instantiating
/// the assets in an LP
/// InstantiateAssetInfo
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
pub struct InstantiateAssetInfo {
    /// Asset Info
    pub info: AssetInfo,
    /// Token address
    pub address: Addr,
    /// Token weight
    pub weight: Uint128,
    /// The minimum amount of profit a position with the asset needs
    /// to be in before closing otherwise, no profit
    pub min_profit_basis_points: Uint128,
    /// Maximum amount of asset that can be held in the LP
    pub max_asset_amount: Uint128,
    /// If the asset is a stable token
    pub is_asset_stable: bool,
    /// If the asset can be shorted 
    pub is_asset_shortable: bool,
    /// Address of the oracle for the asset 
    pub oracle_address: Addr,
    /// Backup oracle address for the asset
    pub backup_oracle_address: Addr,
}


/// This structure describes the parameters used for a message 
/// creating a LP Token. 
/// InstantiateLpMsg
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct InstantiateLpMsg {
    /// Token name
    pub name: String,
    /// Token symbol
    pub symbol: String,
    /// The amount of decimals the token has
    pub decimals: u8,
    /// Initial token balances
    pub initial_balances: Vec<Cw20Coin>,
    /// Minting controls specified in a [`MinterResponse`] structure
    pub mint: Option<MinterResponse>,
}

/// This structure describes a CW20 hook message.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Cw20HookMsg {
    /// Swap a given amount of asset
    Swap {
        belief_price: Option<Decimal>,
        max_spread: Option<Decimal>,
        to: Option<String>,
        ask_asset: AssetInfo,
    },
    /// Withdraw liquidity from the pool
    WithdrawLiquidity { basket_asset: BasketAsset },
}


