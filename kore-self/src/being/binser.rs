//! Track D1b: a small, exact, non-self-describing binary encoding for `serde` types (the payload of a
//! state checkpoint, see `ckpt.rs`).
//!
//! Why not JSON: the folded states use maps with tuple keys (not representable as JSON object keys) and
//! floats that must come back bit for bit (an `f64` is written as its 64 bits, NaN and -0.0 included).
//! Layout (little endian): integers fixed width; `bool` one byte (0 or 1, anything else is an error);
//! `char` as u32; strings and byte strings as u64 length + bytes; `Option` one tag byte; sequences and
//! maps u64 length + items (map: key, value, key, value, ...); structs and tuples their fields in
//! order; enum variants u32 index + their fields. Not self-describing: decoding needs the same Rust
//! type, which is why the checkpoint records a snapshot format version and its kind.
//!
//! Decoding is defensive: every length is checked against the bytes left, trailing bytes are an error,
//! so a truncated or corrupted payload is an error and never a partial value.

use serde::de::{self, DeserializeSeed, IntoDeserializer, Visitor};
use serde::ser::{self, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl ser::Error for Error {
    fn custom<T: fmt::Display>(m: T) -> Self {
        Error(m.to_string())
    }
}
impl de::Error for Error {
    fn custom<T: fmt::Display>(m: T) -> Self {
        Error(m.to_string())
    }
}

type R<T> = Result<T, Error>;

pub fn to_bytes<T: Serialize + ?Sized>(v: &T) -> R<Vec<u8>> {
    let mut s = Ser { out: Vec::new() };
    v.serialize(&mut s)?;
    Ok(s.out)
}

pub fn from_bytes<T: de::DeserializeOwned>(b: &[u8]) -> R<T> {
    let mut d = De { b, pos: 0 };
    let v = T::deserialize(&mut d)?;
    if d.pos != b.len() {
        return Err(Error(format!("{} trailing bytes", b.len() - d.pos)));
    }
    Ok(v)
}

/// `&'static str` fields that only ever hold one of a few literals (`#[serde(with = ...)]`): written as
/// a string, read back as the same literal; any other text is an error (the checkpoint is then ignored).
pub mod static_str {
    /// Every literal such a field holds (World v2 `FamChange::kind`).
    pub const KNOWN: [&str; 2] = ["cell-switch", "prior-conflict"];
    pub fn serialize<S: serde::Serializer>(v: &&'static str, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(v)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<&'static str, D::Error> {
        let t: String = serde::Deserialize::deserialize(d)?;
        KNOWN.iter().copied().find(|k| *k == t).ok_or_else(|| serde::de::Error::custom(format!("unknown literal {t:?}")))
    }
}

// ─── Serializer ──────────────────────────────────────────────────────────────

struct Ser {
    out: Vec<u8>,
}

impl Ser {
    fn len(&mut self, n: usize) {
        self.out.extend_from_slice(&(n as u64).to_le_bytes());
    }
    fn idx(&mut self, i: u32) {
        self.out.extend_from_slice(&i.to_le_bytes());
    }
}

impl<'a> ser::Serializer for &'a mut Ser {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> R<()> {
        self.out.push(v as u8);
        Ok(())
    }
    fn serialize_i8(self, v: i8) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_i16(self, v: i16) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_i32(self, v: i32) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_i64(self, v: i64) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_i128(self, v: i128) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_u8(self, v: u8) -> R<()> {
        self.out.push(v);
        Ok(())
    }
    fn serialize_u16(self, v: u16) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_u32(self, v: u32) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_u64(self, v: u64) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_u128(self, v: u128) -> R<()> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }
    fn serialize_f32(self, v: f32) -> R<()> {
        self.out.extend_from_slice(&v.to_bits().to_le_bytes());
        Ok(())
    }
    fn serialize_f64(self, v: f64) -> R<()> {
        self.out.extend_from_slice(&v.to_bits().to_le_bytes());
        Ok(())
    }
    fn serialize_char(self, v: char) -> R<()> {
        self.serialize_u32(v as u32)
    }
    fn serialize_str(self, v: &str) -> R<()> {
        self.len(v.len());
        self.out.extend_from_slice(v.as_bytes());
        Ok(())
    }
    fn serialize_bytes(self, v: &[u8]) -> R<()> {
        self.len(v.len());
        self.out.extend_from_slice(v);
        Ok(())
    }
    fn serialize_none(self) -> R<()> {
        self.out.push(0);
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> R<()> {
        self.out.push(1);
        v.serialize(self)
    }
    fn serialize_unit(self) -> R<()> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> R<()> {
        Ok(())
    }
    fn serialize_unit_variant(self, _: &'static str, i: u32, _: &'static str) -> R<()> {
        self.idx(i);
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> R<()> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, i: u32, _: &'static str, v: &T) -> R<()> {
        self.idx(i);
        v.serialize(self)
    }
    fn serialize_seq(self, len: Option<usize>) -> R<Self> {
        let n = len.ok_or_else(|| Error("sequence of unknown length".into()))?;
        self.len(n);
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> R<Self> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> R<Self> {
        Ok(self)
    }
    fn serialize_tuple_variant(self, _: &'static str, i: u32, _: &'static str, _: usize) -> R<Self> {
        self.idx(i);
        Ok(self)
    }
    fn serialize_map(self, len: Option<usize>) -> R<Self> {
        let n = len.ok_or_else(|| Error("map of unknown length".into()))?;
        self.len(n);
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> R<Self> {
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, i: u32, _: &'static str, _: usize) -> R<Self> {
        self.idx(i);
        Ok(self)
    }
    fn is_human_readable(&self) -> bool {
        false
    }
}

impl<'a> ser::SerializeSeq for &'a mut Ser {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> R<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> R<()> {
        Ok(())
    }
}
impl<'a> ser::SerializeTuple for &'a mut Ser {
    type Ok = ();
    type Error = Error;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> R<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> R<()> {
        Ok(())
    }
}
impl<'a> ser::SerializeTupleStruct for &'a mut Ser {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> R<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> R<()> {
        Ok(())
    }
}
impl<'a> ser::SerializeTupleVariant for &'a mut Ser {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> R<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> R<()> {
        Ok(())
    }
}
impl<'a> ser::SerializeMap for &'a mut Ser {
    type Ok = ();
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, k: &T) -> R<()> {
        k.serialize(&mut **self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> R<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> R<()> {
        Ok(())
    }
}
impl<'a> ser::SerializeStruct for &'a mut Ser {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> R<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> R<()> {
        Ok(())
    }
}
impl<'a> ser::SerializeStructVariant for &'a mut Ser {
    type Ok = ();
    type Error = Error;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, _: &'static str, v: &T) -> R<()> {
        v.serialize(&mut **self)
    }
    fn end(self) -> R<()> {
        Ok(())
    }
}

// ─── Deserializer ────────────────────────────────────────────────────────────

struct De<'b> {
    b: &'b [u8],
    pos: usize,
}

impl<'b> De<'b> {
    fn take(&mut self, n: usize) -> R<&'b [u8]> {
        if self.b.len() - self.pos < n {
            return Err(Error("unexpected end of payload".into()));
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn arr<const N: usize>(&mut self) -> R<[u8; N]> {
        let mut a = [0u8; N];
        a.copy_from_slice(self.take(N)?);
        Ok(a)
    }
    /// A length; at least `min_item` bytes per item must remain (guards against huge bogus lengths).
    fn len(&mut self, min_item: usize) -> R<usize> {
        let n = u64::from_le_bytes(self.arr()?);
        let left = (self.b.len() - self.pos) as u64;
        let limit = if min_item == 0 { left.saturating_add(1 << 20) } else { left / min_item as u64 };
        if n > limit {
            return Err(Error(format!("length {n} exceeds the payload")));
        }
        Ok(n as usize)
    }
    fn idx(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
}

macro_rules! de_num {
    ($name:ident, $visit:ident, $t:ty) => {
        fn $name<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
            v.$visit(<$t>::from_le_bytes(self.arr()?))
        }
    };
}

impl<'de, 'a, 'b> de::Deserializer<'de> for &'a mut De<'b> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, _: V) -> R<V::Value> {
        Err(Error("the checkpoint encoding is not self-describing".into()))
    }
    fn deserialize_bool<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        match self.take(1)?[0] {
            0 => v.visit_bool(false),
            1 => v.visit_bool(true),
            x => Err(Error(format!("bad bool byte {x}"))),
        }
    }
    de_num!(deserialize_i8, visit_i8, i8);
    de_num!(deserialize_i16, visit_i16, i16);
    de_num!(deserialize_i32, visit_i32, i32);
    de_num!(deserialize_i64, visit_i64, i64);
    de_num!(deserialize_i128, visit_i128, i128);
    de_num!(deserialize_u8, visit_u8, u8);
    de_num!(deserialize_u16, visit_u16, u16);
    de_num!(deserialize_u32, visit_u32, u32);
    de_num!(deserialize_u64, visit_u64, u64);
    de_num!(deserialize_u128, visit_u128, u128);
    fn deserialize_f32<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        v.visit_f32(f32::from_bits(u32::from_le_bytes(self.arr()?)))
    }
    fn deserialize_f64<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        v.visit_f64(f64::from_bits(u64::from_le_bytes(self.arr()?)))
    }
    fn deserialize_char<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        let c = u32::from_le_bytes(self.arr()?);
        v.visit_char(char::from_u32(c).ok_or_else(|| Error(format!("bad char {c}")))?)
    }
    fn deserialize_str<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        let n = self.len(1)?;
        let s = std::str::from_utf8(self.take(n)?).map_err(|_| Error("string is not UTF-8".into()))?;
        v.visit_str(s)
    }
    fn deserialize_string<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        self.deserialize_str(v)
    }
    fn deserialize_bytes<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        let n = self.len(1)?;
        v.visit_bytes(self.take(n)?)
    }
    fn deserialize_byte_buf<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        self.deserialize_bytes(v)
    }
    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        match self.take(1)?[0] {
            0 => v.visit_none(),
            1 => v.visit_some(self),
            x => Err(Error(format!("bad option tag {x}"))),
        }
    }
    fn deserialize_unit<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        v.visit_unit()
    }
    fn deserialize_unit_struct<V: Visitor<'de>>(self, _: &'static str, v: V) -> R<V::Value> {
        v.visit_unit()
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(self, _: &'static str, v: V) -> R<V::Value> {
        v.visit_newtype_struct(self)
    }
    fn deserialize_seq<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        // an item may be zero bytes (unit), so only a loose bound is possible here
        let n = self.len(0)?;
        v.visit_seq(Items { de: self, left: n })
    }
    fn deserialize_tuple<V: Visitor<'de>>(self, n: usize, v: V) -> R<V::Value> {
        v.visit_seq(Items { de: self, left: n })
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(self, _: &'static str, n: usize, v: V) -> R<V::Value> {
        v.visit_seq(Items { de: self, left: n })
    }
    fn deserialize_map<V: Visitor<'de>>(self, v: V) -> R<V::Value> {
        let n = self.len(0)?;
        v.visit_map(Items { de: self, left: n })
    }
    fn deserialize_struct<V: Visitor<'de>>(self, _: &'static str, fields: &'static [&'static str], v: V) -> R<V::Value> {
        v.visit_seq(Items { de: self, left: fields.len() })
    }
    fn deserialize_enum<V: Visitor<'de>>(self, _: &'static str, _: &'static [&'static str], v: V) -> R<V::Value> {
        v.visit_enum(self)
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, _: V) -> R<V::Value> {
        Err(Error("identifiers are not encoded".into()))
    }
    fn deserialize_ignored_any<V: Visitor<'de>>(self, _: V) -> R<V::Value> {
        Err(Error("the checkpoint encoding cannot skip values".into()))
    }
    fn is_human_readable(&self) -> bool {
        false
    }
}

struct Items<'a, 'b> {
    de: &'a mut De<'b>,
    left: usize,
}

impl<'de, 'a, 'b> de::SeqAccess<'de> for Items<'a, 'b> {
    type Error = Error;
    fn next_element_seed<T: DeserializeSeed<'de>>(&mut self, seed: T) -> R<Option<T::Value>> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(&mut *self.de).map(Some)
    }
    fn size_hint(&self) -> Option<usize> {
        // bounded so that a corrupted length cannot reserve a huge buffer up front
        Some(self.left.min(4096))
    }
}

impl<'de, 'a, 'b> de::MapAccess<'de> for Items<'a, 'b> {
    type Error = Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> R<Option<K::Value>> {
        if self.left == 0 {
            return Ok(None);
        }
        self.left -= 1;
        seed.deserialize(&mut *self.de).map(Some)
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> R<V::Value> {
        seed.deserialize(&mut *self.de)
    }
    fn size_hint(&self) -> Option<usize> {
        Some(self.left.min(4096))
    }
}

impl<'de, 'a, 'b> de::EnumAccess<'de> for &'a mut De<'b> {
    type Error = Error;
    type Variant = Self;
    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> R<(V::Value, Self)> {
        let i = self.idx()?;
        let v = seed.deserialize(i.into_deserializer())?;
        Ok((v, self))
    }
}

impl<'de, 'a, 'b> de::VariantAccess<'de> for &'a mut De<'b> {
    type Error = Error;
    fn unit_variant(self) -> R<()> {
        Ok(())
    }
    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> R<T::Value> {
        seed.deserialize(self)
    }
    fn tuple_variant<V: Visitor<'de>>(self, n: usize, v: V) -> R<V::Value> {
        v.visit_seq(Items { de: self, left: n })
    }
    fn struct_variant<V: Visitor<'de>>(self, fields: &'static [&'static str], v: V) -> R<V::Value> {
        v.visit_seq(Items { de: self, left: fields.len() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    enum E {
        A,
        B(u8, String),
        C { x: f64 },
    }

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct S {
        a: Option<i64>,
        m: BTreeMap<(u8, u8), Vec<E>>,
        q: [VecDeque<String>; 3],
        f: f64,
        u: (),
        b: bool,
    }

    fn sample() -> S {
        let mut m = BTreeMap::new();
        m.insert((1, 2), vec![E::A, E::B(7, "x".into()), E::C { x: -0.0 }]);
        m.insert((0, 9), vec![E::C { x: f64::NAN }]);
        S { a: Some(-5), m, q: [VecDeque::from(vec!["a".to_string()]), VecDeque::new(), VecDeque::from(vec!["b".into(), "c".into()])], f: 0.1 + 0.2, u: (), b: true }
    }

    #[test]
    fn round_trip_is_exact_including_nan_and_negative_zero() {
        let s = sample();
        let b = to_bytes(&s).unwrap();
        let t: S = from_bytes(&b).unwrap();
        assert_eq!(format!("{s:?}"), format!("{t:?}"));
        assert_eq!(to_bytes(&t).unwrap(), b);
        let E::C { x } = &t.m[&(1, 2)][2] else { panic!() };
        assert!(x.is_sign_negative() && *x == 0.0);
        assert!(matches!(t.m[&(0, 9)][0], E::C { x } if x.is_nan()));
    }

    #[test]
    fn truncated_corrupted_or_padded_payloads_are_errors() {
        let b = to_bytes(&sample()).unwrap();
        for n in 0..b.len() {
            assert!(from_bytes::<S>(&b[..n]).is_err(), "truncated at {n}");
        }
        let mut long = b.clone();
        long.push(0);
        assert!(from_bytes::<S>(&long).is_err());
        let mut bad = b.clone();
        bad[0] = 7; // option tag
        assert!(from_bytes::<S>(&bad).is_err());
        let mut huge = b;
        huge[9..17].copy_from_slice(&u64::MAX.to_le_bytes()); // the map length
        assert!(from_bytes::<S>(&huge).is_err());
    }
}
