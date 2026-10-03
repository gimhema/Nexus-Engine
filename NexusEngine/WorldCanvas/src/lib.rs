//! WorldCanvas — WorldEditor 용 픽셀 스프라이트 저작 도구.
//!
//! 원본은 텍스트(`.canvas`)다. 한 글자가 한 픽셀이고, 프레임은 다른 프레임에 연산을 쌓아
//! 파생시킬 수 있다. 원본에서 시트 PNG 와 WorldEditor 시트 정의(`.sheet.ron`)를 만든다.
//!
//! 흐름: [`parse`](parse::parse) → [`Resolver`](resolve::Resolver) → [`export`] / [`preview`].

#![forbid(unsafe_code)]

pub mod bitmap;
pub mod color;
pub mod doc;
pub mod error;
pub mod export;
pub mod import;
pub mod parse;
pub mod preview;
pub mod resolve;
