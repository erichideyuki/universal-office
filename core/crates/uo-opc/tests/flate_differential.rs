//! Differential test against fflate 0.8.3 on 2,000 mutated DEFLATE streams (random byte changes
//! and truncations of four base streams: dynamic, fixed, stored-truncated and stored). Each case
//! is inflated without and with a declared size; fflate's result (length and CRC-32 of the
//! output, or the error message) was recorded by `node` with the same xorshift32 mutations.
//!
//! Accepted by name: D1 (gate 2), an empty stream with a declared size > 0 is `Corrupt` where
//! fflate returns that many zeros. Every case also runs `check_raw` (the window mode of D5),
//! which must describe exactly what `inflate_raw` returns.
#![allow(clippy::unwrap_used)] // helpers outside #[test] functions

use uo_opc::Error;
use uo_opc::flate;

const BASES: [&str; 4] = [
    "b329b72ab003124520a2c4ce233527275f47a13cbf28274551c1c0d0c8d8c4d4ccdcc2d2461f240b22810a812458cfa8465c1aab2a2bcacb4a4b8a8b0ab1b100",
    "2bcec82f2a512849ad280100",
    "0174018bfe3c773a703e3c773a723e3c773a743e48656c6c6f2c20776f726c642120303132333435363738393c2f773a743e3c2f773a723e3c2f773a703e3c773a703e3c773a723e3c773a743e48656c6c6f2c20776f726c642120303132333435363738393c2f773a743e3c2f773a723e3c2f773a703e3c",
    "013c00c3ff3c773a703e3c773a723e3c773a743e48656c6c6f2c20776f726c642120303132333435363738393c2f773a743e3c2f773a723e3c2f773a703e3c773a",
];
const EXPECTED: &str = "ok:477:8225b0f2|ok:465:b0f338c7
ok:10:6293f9c9|ok:10:6293f9c9
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:373:88b56515|ok:87:2518875
ok:10:3ce8bd3c|ok:10:3ce8bd3c
err:unexpected EOF|err:unexpected EOF
ok:60:38d6993e|ok:60:38d6993e
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:654ce3b|ok:60:654ce3b
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:cc085b7|ok:60:cc085b7
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:86879354|err:offset is out of bounds
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3730e223|ok:60:3730e223
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:a161391d|ok:60:a161391d
ok:372:daf4acaa|ok:101:10eecc93
ok:0:0|ok:338:f18eaf56
err:unexpected EOF|err:unexpected EOF
ok:60:e40c3ee3|ok:60:e40c3ee3
ok:472:6adcc851|ok:472:6adcc851
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:cf3bed58|ok:314:866b892a
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:82b8afbc|err:offset is out of bounds
ok:372:ccbd91b8|ok:281:a1d568f7
ok:10:5e47c234|ok:10:5e47c234
err:unexpected EOF|err:unexpected EOF
ok:60:423ba4c6|ok:60:423ba4c6
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:41c57839|ok:60:41c57839
ok:0:0|ok:399:44d90589
ok:10:afab80b3|ok:10:afab80b3
err:unexpected EOF|err:unexpected EOF
ok:60:974483df|ok:60:974483df
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:7b5e9314|ok:60:7b5e9314
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:4265db86|ok:60:4265db86
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:2c90ac44|ok:60:2c90ac44
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d3af0b9a|ok:60:d3af0b9a
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:5c7871f9|ok:60:5c7871f9
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:ce0fa7ab|ok:60:ce0fa7ab
err:invalid distance|err:invalid distance
ok:10:2288ed1c|ok:10:2288ed1c
err:unexpected EOF|err:unexpected EOF
ok:60:10c18200|err:offset is out of bounds
err:invalid length/literal|err:invalid length/literal
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:beb28281|ok:60:beb28281
ok:5:93b93ea4|ok:5:93b93ea4
ok:10:88fb8c00|ok:10:88fb8c00
err:unexpected EOF|err:unexpected EOF
ok:60:dc875d55|ok:60:dc875d55
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3a858158|ok:60:3a858158
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:bf8b6486|ok:60:bf8b6486
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:7ea5e1b4|ok:60:7ea5e1b4
ok:371:cc7b33b1|ok:371:cc7b33b1
ok:10:662a8c3|ok:10:662a8c3
err:unexpected EOF|err:unexpected EOF
ok:60:bd3c1bd8|ok:60:bd3c1bd8
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:b38974a|ok:60:b38974a
ok:372:56244483|ok:372:56244483
ok:10:2d3a7d23|ok:10:2d3a7d23
err:unexpected EOF|err:unexpected EOF
ok:60:5737713d|ok:60:5737713d
ok:372:1b757a80|ok:372:1b757a80
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:54:977830f6|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
ok:10:33195eb8|ok:10:33195eb8
err:unexpected EOF|err:unexpected EOF
ok:60:9d903d5a|ok:60:9d903d5a
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:4f34429e|err:offset is out of bounds
ok:372:e0b44c98|ok:372:e0b44c98
ok:0:0|ok:34:41716c2a
err:unexpected EOF|err:unexpected EOF
ok:60:8b378440|ok:60:8b378440
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
ok:10:79471e25|ok:10:79471e25
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
ok:10:b59be5da|ok:10:b59be5da
err:unexpected EOF|err:unexpected EOF
ok:60:98509098|ok:60:98509098
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:810f6dc7|ok:60:810f6dc7
ok:116:e2dfdc8f|ok:116:e2dfdc8f
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:ec6387c6|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:d3c6139a|ok:10:d3c6139a
err:unexpected EOF|err:unexpected EOF
ok:60:834cde32|ok:60:834cde32
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:8a41340f|ok:60:8a41340f
ok:372:e13f976f|ok:345:bce5723e
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:b961a3c|ok:60:b961a3c
ok:471:ba4cce82|ok:226:70d3b1bf
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:7df0da53|ok:60:7df0da53
err:invalid block type|err:invalid block type
ok:10:9afd7b07|ok:10:9afd7b07
err:unexpected EOF|err:unexpected EOF
ok:60:1cbafa51|ok:60:1cbafa51
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:af9ff1d0|ok:60:af9ff1d0
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:b6c6f5ed|ok:60:b6c6f5ed
err:unexpected EOF|err:unexpected EOF
ok:0:0|ok:179:f399740e
err:unexpected EOF|err:unexpected EOF
ok:60:efc7ff60|ok:60:efc7ff60
err:invalid distance|err:invalid distance
ok:10:f9e4637c|ok:10:f9e4637c
err:unexpected EOF|err:unexpected EOF
ok:60:29e8dc91|err:offset is out of bounds
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:c0dc3861|ok:60:c0dc3861
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:10:5be63cb3|ok:10:5be63cb3
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:eae99d54|ok:10:eae99d54
err:unexpected EOF|err:unexpected EOF
ok:60:bf11bb9e|ok:60:bf11bb9e
ok:372:923832ea|ok:20:ac78a104
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:14eb2a89|ok:60:14eb2a89
err:invalid distance|err:invalid distance
ok:10:c6e7b690|ok:10:c6e7b690
err:unexpected EOF|err:unexpected EOF
ok:60:dd3d748f|ok:60:dd3d748f
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:5b41a2d7|ok:60:5b41a2d7
ok:372:19481274|ok:206:8e190dac
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:60e432b5|ok:60:60e432b5
err:invalid distance|err:invalid distance
ok:52:beb83e73|ok:52:beb83e73
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:543cf0ac|ok:60:543cf0ac
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:0:0|ok:563:11af598b
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:7656800|ok:60:7656800
err:invalid distance|err:invalid distance
ok:10:121f5925|ok:10:121f5925
err:unexpected EOF|err:unexpected EOF
ok:60:ed141136|ok:60:ed141136
err:invalid distance|err:invalid distance
ok:10:43346413|ok:10:43346413
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:423ba4c6|ok:60:423ba4c6
ok:379:83404582|ok:250:8f8d5c67
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e866ae7f|ok:60:e866ae7f
err:invalid distance|err:invalid distance
ok:10:1bc61ab|ok:10:1bc61ab
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:791ecc54|ok:10:791ecc54
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:6a17c6f2|ok:370:e4c563f9
ok:10:ad58772c|ok:10:ad58772c
err:unexpected EOF|err:unexpected EOF
ok:60:423ba4c6|ok:60:423ba4c6
err:invalid distance|err:invalid distance
ok:10:a1bd5bb0|ok:10:a1bd5bb0
err:unexpected EOF|err:unexpected EOF
ok:60:981c97c3|ok:60:981c97c3
err:invalid distance|err:invalid distance
ok:10:d71977d0|ok:10:d71977d0
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:55dbade1|ok:60:55dbade1
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:f4f3eea1|err:offset is out of bounds
ok:372:cbbc69dd|ok:105:50104e18
ok:10:66d7cac1|ok:10:66d7cac1
err:unexpected EOF|err:unexpected EOF
ok:60:8f4afefb|ok:60:8f4afefb
ok:372:b7b1fe02|ok:372:b7b1fe02
ok:10:f1291a3c|ok:10:f1291a3c
err:unexpected EOF|err:unexpected EOF
ok:60:c2634a9f|ok:60:c2634a9f
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
ok:372:8601d0f8|ok:372:8601d0f8
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:b1d865cd|ok:60:b1d865cd
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:f1e4df88|ok:60:f1e4df88
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:2b7b0077|ok:60:2b7b0077
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:ce2a4278|ok:60:ce2a4278
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:28d7868a|ok:60:28d7868a
ok:372:8b19d416|ok:282:5aca8a2
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:9357fb24|err:offset is out of bounds
err:invalid distance|err:invalid distance
ok:8:b6864c0c|ok:8:b6864c0c
err:unexpected EOF|err:unexpected EOF
ok:60:cc03a68c|ok:60:cc03a68c
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:4f902381|ok:10:4f902381
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:740171fb|ok:372:740171fb
ok:1:82079eb1|ok:1:82079eb1
err:unexpected EOF|err:unexpected EOF
ok:60:b4dbef8|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
ok:10:c4d89baa|ok:10:c4d89baa
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:10:ec23c5e7|ok:10:ec23c5e7
err:unexpected EOF|err:unexpected EOF
ok:60:29e0e9d|ok:60:29e0e9d
ok:372:dc4fc115|ok:176:a12b3af3
ok:0:0|ok:137:9b1c9bf3
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:0:0|ok:464:c1b939ef
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:ff685ac|ok:60:ff685ac
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3344c45c|ok:60:3344c45c
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d64834c3|ok:60:d64834c3
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:a3882d17|ok:60:a3882d17
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:ab559393|ok:60:ab559393
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:650af9f4|ok:60:650af9f4
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:5e332fad|ok:60:5e332fad
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:6e8c2b4b|ok:60:6e8c2b4b
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:10:210c747b|ok:10:210c747b
err:unexpected EOF|err:unexpected EOF
ok:60:7b183514|ok:60:7b183514
err:invalid length/literal|err:invalid length/literal
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:ff58002f|ok:60:ff58002f
ok:372:17f9fcb6|ok:372:17f9fcb6
ok:10:660a84d4|ok:10:660a84d4
err:unexpected EOF|err:unexpected EOF
ok:60:2bb09c02|ok:60:2bb09c02
err:invalid distance|err:invalid distance
ok:10:85d43d33|ok:10:85d43d33
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:25d87080|ok:372:25d87080
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:c348e1be|ok:60:c348e1be
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:37a53f81|ok:60:37a53f81
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:71bb4e2f|ok:60:71bb4e2f
ok:372:d31e1ae3|ok:372:d31e1ae3
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:ff3119b2|ok:60:ff3119b2
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
ok:10:5fc2f0e|ok:10:5fc2f0e
err:unexpected EOF|err:unexpected EOF
ok:60:fe996142|ok:60:fe996142
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:7caf798a|ok:60:7caf798a
ok:372:cfea2211|ok:372:cfea2211
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:63720191|ok:60:63720191
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:4c43c9b5|ok:60:4c43c9b5
err:unexpected EOF|err:unexpected EOF
ok:10:9e0c29e4|ok:10:9e0c29e4
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:5ce77c8c|ok:133:3f4e28e7
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e53febad|ok:60:e53febad
err:invalid distance|err:invalid distance
ok:10:96c003b4|ok:10:96c003b4
err:unexpected EOF|err:unexpected EOF
ok:60:d0c1602f|ok:60:d0c1602f
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:invalid length/literal|err:invalid length/literal
ok:372:628d28b|ok:372:628d28b
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:b37cb52a|ok:60:b37cb52a
err:invalid distance|err:invalid distance
ok:10:1c23f4fc|ok:10:1c23f4fc
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:c2bbb0a4|ok:60:c2bbb0a4
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:5fc91713|ok:60:5fc91713
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:cb72ee2e|ok:60:cb72ee2e
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:f87dd037|ok:60:f87dd037
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:cfbd1f33|ok:60:cfbd1f33
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:80f8b23a|ok:60:80f8b23a
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:edd7737f|ok:60:edd7737f
ok:372:6a62d9be|ok:372:6a62d9be
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:857398f4|ok:60:857398f4
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid length/literal|err:invalid length/literal
ok:116:8170f64|ok:116:8170f64
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:9af1ac62|ok:60:9af1ac62
err:invalid distance|err:invalid distance
ok:10:47cf2bb7|ok:10:47cf2bb7
err:unexpected EOF|err:unexpected EOF
ok:60:397eda93|ok:60:397eda93
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:1b9a7353|ok:60:1b9a7353
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d2399ec7|ok:60:d2399ec7
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:4f17e7f7|err:offset is out of bounds
ok:372:3328faa9|ok:372:3328faa9
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e9c7d2d6|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
ok:10:20ce1e4c|ok:10:20ce1e4c
err:unexpected EOF|err:unexpected EOF
ok:60:3ea86430|ok:60:3ea86430
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:14d15a45|ok:340:bffba7e9
ok:10:fd5d0610|ok:10:fd5d0610
err:unexpected EOF|err:unexpected EOF
ok:60:fa2e6332|ok:60:fa2e6332
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:6d85b4bc|ok:60:6d85b4bc
ok:372:c367ae38|ok:372:c367ae38
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:2c3e090d|ok:10:2c3e090d
err:unexpected EOF|err:unexpected EOF
ok:60:e2446699|ok:60:e2446699
err:invalid distance|err:invalid distance
ok:10:e9897e62|ok:10:e9897e62
err:unexpected EOF|err:unexpected EOF
ok:60:6cb82195|ok:60:6cb82195
err:invalid distance|err:invalid distance
ok:10:18ec3111|ok:10:18ec3111
err:unexpected EOF|err:unexpected EOF
ok:60:dc7e6f8c|ok:60:dc7e6f8c
err:invalid distance|err:invalid distance
ok:10:a04bf49e|ok:10:a04bf49e
err:unexpected EOF|err:unexpected EOF
ok:60:e0f01adb|ok:60:e0f01adb
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:2c163de8|ok:60:2c163de8
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:5d814d3f|ok:60:5d814d3f
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:423ba4c6|ok:60:423ba4c6
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:423ba4c6|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:31be6c85|ok:60:31be6c85
err:invalid distance|err:invalid distance
ok:10:51281d7e|ok:10:51281d7e
err:unexpected EOF|err:unexpected EOF
ok:60:878e6fee|ok:60:878e6fee
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:5b4d9e84|ok:159:b3d04b53
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:fabadeb3|ok:60:fabadeb3
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:5592f95a|ok:60:5592f95a
ok:372:962f4033|ok:220:12e74a23
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:53159a5a|ok:60:53159a5a
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:158b9358|ok:60:158b9358
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1c37f7af|ok:60:1c37f7af
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:771533d4|ok:60:771533d4
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:8c0adc9b|ok:60:8c0adc9b
ok:372:36187d96|ok:372:36187d96
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:fde42e7f|ok:60:fde42e7f
ok:372:ffe2d8|ok:287:9f2116cb
ok:10:35ce955f|ok:10:35ce955f
err:unexpected EOF|err:unexpected EOF
ok:60:2c9de37d|ok:60:2c9de37d
ok:383:c2de1e2f|ok:226:8ebc201b
ok:0:0|ok:375:cd11571d
err:unexpected EOF|err:unexpected EOF
ok:60:4fc5fe76|ok:60:4fc5fe76
ok:376:9ab0616c|ok:223:42207ec5
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:b0fd5c2a|ok:60:b0fd5c2a
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:bcfc4420|ok:60:bcfc4420
ok:372:73d1a486|ok:297:d14a369d
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:9bf74869|ok:60:9bf74869
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:b6c5c332|ok:60:b6c5c332
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d58b7a00|ok:60:d58b7a00
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e0980fd3|ok:60:e0980fd3
err:invalid distance|err:invalid distance
ok:10:64e59bb|ok:10:64e59bb
err:unexpected EOF|err:unexpected EOF
ok:60:6ade2ed5|ok:60:6ade2ed5
err:invalid distance|err:invalid distance
ok:5:d508a4bb|ok:5:d508a4bb
err:unexpected EOF|err:unexpected EOF
ok:60:ee480efe|ok:60:ee480efe
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:f6a6d0c6|ok:60:f6a6d0c6
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:7bf22bce|ok:372:7bf22bce
ok:10:4800802a|ok:10:4800802a
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:62eb6de1|ok:10:62eb6de1
err:unexpected EOF|err:unexpected EOF
ok:60:284f2c8d|ok:60:284f2c8d
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:423ba4c6|ok:60:423ba4c6
ok:372:8d5604fd|ok:123:2794fedb
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:65cc98e2|ok:60:65cc98e2
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:52b9a667|ok:60:52b9a667
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:948ac306|ok:60:948ac306
ok:5:93b93ea4|ok:5:93b93ea4
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:7d51f50a|ok:60:7d51f50a
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1c9e8ba1|ok:60:1c9e8ba1
ok:372:b29d5cf9|ok:7:f7417860
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3321b759|err:offset is out of bounds
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:44:6a50e314|ok:44:6a50e314
err:unexpected EOF|err:unexpected EOF
ok:10:e2687e44|ok:10:e2687e44
err:unexpected EOF|err:unexpected EOF
ok:60:c6691cfe|ok:60:c6691cfe
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:534bfb34|ok:60:534bfb34
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:9a0c1821|ok:60:9a0c1821
ok:372:ab9c8c55|ok:189:be0e6bc6
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:98f87419|ok:60:98f87419
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:cc74599f|ok:60:cc74599f
err:invalid distance|err:invalid distance
ok:10:31bf7fb|ok:10:31bf7fb
err:unexpected EOF|err:unexpected EOF
ok:60:b02698d8|ok:60:b02698d8
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:8235c151|ok:60:8235c151
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:824486da|ok:60:824486da
err:invalid distance|err:invalid distance
ok:10:718dbcef|ok:10:718dbcef
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:0:0|ok:454:215d1c6e
err:unexpected EOF|err:unexpected EOF
ok:60:4d2134a9|ok:60:4d2134a9
err:invalid distance|err:invalid distance
ok:10:79eb2112|ok:10:79eb2112
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:b33ecb2f|ok:60:b33ecb2f
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:e9863746|ok:60:e9863746
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:a164fa73|ok:60:a164fa73
err:unexpected EOF|err:unexpected EOF
ok:0:0|ok:666:5e148db
err:unexpected EOF|err:unexpected EOF
ok:60:a34e2bb2|ok:60:a34e2bb2
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:b50adfe2|ok:60:b50adfe2
err:invalid block type|err:invalid block type
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:20447ce8|ok:10:20447ce8
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d91aa061|ok:60:d91aa061
err:invalid distance|err:invalid distance
ok:10:e77b423b|ok:10:e77b423b
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:73cf5c89|ok:372:73cf5c89
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e956eff7|ok:60:e956eff7
err:unexpected EOF|err:unexpected EOF
ok:132:e55ffd58|ok:132:e55ffd58
err:unexpected EOF|err:unexpected EOF
ok:60:772b9abc|ok:60:772b9abc
ok:372:f9407fe2|ok:372:f9407fe2
ok:0:0|ok:455:7829fece
err:unexpected EOF|err:unexpected EOF
ok:60:47699643|ok:60:47699643
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:f78df3bc|ok:60:f78df3bc
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:c622e2aa|ok:60:c622e2aa
err:unexpected EOF|err:unexpected EOF
ok:10:7c95dbc8|ok:10:7c95dbc8
err:unexpected EOF|err:unexpected EOF
ok:60:47905b09|ok:60:47905b09
err:invalid distance|err:invalid distance
ok:10:3f3f8172|ok:10:3f3f8172
err:unexpected EOF|err:unexpected EOF
ok:60:351d751d|ok:60:351d751d
err:unexpected EOF|err:unexpected EOF
ok:10:814c2651|ok:10:814c2651
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:71632fbc|ok:60:71632fbc
ok:372:69319d5f|ok:58:c0ea083b
ok:10:149dcad7|ok:10:149dcad7
err:unexpected EOF|err:unexpected EOF
ok:60:278c220d|ok:60:278c220d
err:unexpected EOF|err:unexpected EOF
ok:17:3aa64ed6|ok:17:3aa64ed6
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d4362830|ok:60:d4362830
err:invalid distance|err:invalid distance
ok:10:bc114a0e|ok:10:bc114a0e
err:unexpected EOF|err:unexpected EOF
ok:60:b5d6188e|ok:60:b5d6188e
ok:372:28daf65|ok:372:28daf65
ok:10:e868a9cf|ok:10:e868a9cf
err:unexpected EOF|err:unexpected EOF
ok:60:c7ad3076|ok:60:c7ad3076
err:invalid distance|err:invalid distance
ok:10:5b519a5|ok:10:5b519a5
err:unexpected EOF|err:unexpected EOF
ok:60:7fe96e8f|ok:60:7fe96e8f
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:7269994|ok:10:7269994
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:aa4b9853|ok:60:aa4b9853
err:invalid distance|err:invalid distance
ok:10:51cc6a7d|ok:10:51cc6a7d
err:unexpected EOF|err:unexpected EOF
ok:60:c6b122d0|ok:60:c6b122d0
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:f35f32f5|ok:60:f35f32f5
err:invalid distance|err:invalid distance
ok:10:9b53d47c|ok:10:9b53d47c
err:unexpected EOF|err:unexpected EOF
ok:60:75df1bd7|ok:60:75df1bd7
ok:372:e9c544e1|ok:372:e9c544e1
ok:10:491b056c|ok:10:491b056c
err:unexpected EOF|err:unexpected EOF
ok:60:7ddd2aa8|ok:60:7ddd2aa8
ok:372:3f0a2f17|ok:126:eefd8dc7
ok:10:a1b6d925|ok:10:a1b6d925
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:59dffdcf|ok:60:59dffdcf
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:533906dc|ok:60:533906dc
ok:498:f868df51|ok:498:f868df51
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:6489fb1|ok:60:6489fb1
err:invalid distance|err:invalid distance
ok:10:a03f3fc9|ok:10:a03f3fc9
err:unexpected EOF|err:unexpected EOF
ok:60:2bf196ee|ok:60:2bf196ee
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:26f7babd|ok:60:26f7babd
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:4f6845b9|ok:60:4f6845b9
ok:372:6807defe|ok:372:6807defe
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:6a5298d6|ok:60:6a5298d6
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:20ce1e4c|ok:10:20ce1e4c
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:40f6aa5e|ok:60:40f6aa5e
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3818157c|ok:60:3818157c
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:8fe12be0|err:offset is out of bounds
ok:372:63f809da|ok:372:63f809da
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:94a7b36d|ok:60:94a7b36d
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1d972d37|ok:60:1d972d37
ok:372:cbd20f27|ok:372:cbd20f27
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:fe22e87|ok:60:fe22e87
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:5d237646|ok:60:5d237646
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:4a9c267|ok:60:4a9c267
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:4190f969|ok:60:4190f969
err:invalid distance|err:invalid distance
ok:5:f82fa034|ok:5:f82fa034
err:unexpected EOF|err:unexpected EOF
ok:60:65d696b3|ok:60:65d696b3
ok:372:c6a7d5a7|ok:372:c6a7d5a7
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:1fcf344f|ok:60:1fcf344f
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3d92fbe2|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:0:0|ok:307:30dbcbf1
ok:60:8bf1c926|ok:60:8bf1c926
err:invalid distance|err:invalid distance
ok:10:c44483cb|ok:10:c44483cb
err:unexpected EOF|err:unexpected EOF
ok:60:8428fee7|ok:60:8428fee7
err:invalid distance|err:invalid distance
ok:24:b6ac0d9|ok:24:b6ac0d9
err:unexpected EOF|err:unexpected EOF
ok:60:87d2217b|ok:60:87d2217b
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:f1b5e198|ok:60:f1b5e198
err:unexpected EOF|err:unexpected EOF
ok:10:2c256551|ok:10:2c256551
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:ea75f31e|ok:60:ea75f31e
ok:372:dd27e7e2|ok:372:dd27e7e2
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:adc00a1|ok:60:adc00a1
ok:30:9926e5b|ok:30:9926e5b
ok:1:f26d6a3e|ok:1:f26d6a3e
err:unexpected EOF|err:unexpected EOF
ok:60:22dbf249|ok:60:22dbf249
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:c863246b|ok:60:c863246b
ok:372:b60e445d|ok:227:334be86e
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:c9db7369|ok:60:c9db7369
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3b1f9854|ok:60:3b1f9854
ok:372:5702e29e|ok:372:5702e29e
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:4b349e04|ok:60:4b349e04
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d0165a78|ok:60:d0165a78
ok:372:e994572|ok:204:c3897a3a
ok:10:488aa4e5|ok:10:488aa4e5
err:unexpected EOF|err:unexpected EOF
ok:60:19f27968|ok:60:19f27968
err:invalid distance|err:invalid distance
ok:95:674fc8cc|ok:95:674fc8cc
err:unexpected EOF|err:unexpected EOF
ok:60:b6f4a985|ok:60:b6f4a985
ok:372:6586132f|ok:372:6586132f
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:6cf07766|ok:60:6cf07766
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e6960068|ok:60:e6960068
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:ae1cd4c4|ok:60:ae1cd4c4
err:invalid distance|err:invalid distance
ok:10:9adfb42b|ok:10:9adfb42b
err:unexpected EOF|err:unexpected EOF
ok:60:5895b12e|ok:60:5895b12e
err:unexpected EOF|err:unexpected EOF
ok:10:a9f45609|ok:10:a9f45609
err:unexpected EOF|err:unexpected EOF
ok:60:4d1080c|ok:60:4d1080c
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:5c7331f5|ok:60:5c7331f5
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:644a194f|ok:60:644a194f
ok:376:54538024|ok:312:6911214b
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:23164186|ok:60:23164186
ok:570:33f9f562|ok:540:cadf5407
ok:10:20ce1e4c|ok:10:20ce1e4c
err:unexpected EOF|err:unexpected EOF
ok:60:6f9d219d|ok:60:6f9d219d
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:9a7d0b0d|ok:60:9a7d0b0d
err:invalid distance|err:invalid distance
ok:10:6d7c7f14|ok:10:6d7c7f14
err:unexpected EOF|err:unexpected EOF
ok:60:f172eae1|ok:60:f172eae1
err:unexpected EOF|err:unexpected EOF
ok:109:660061f7|ok:66:2d4c29cb
err:unexpected EOF|err:unexpected EOF
ok:60:e904d590|ok:60:e904d590
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:15aad078|ok:60:15aad078
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:90450950|ok:60:90450950
ok:372:d3811cd8|ok:372:d3811cd8
ok:10:1573f5a1|ok:10:1573f5a1
err:unexpected EOF|err:unexpected EOF
ok:60:c43d609c|ok:60:c43d609c
err:invalid distance|err:invalid distance
ok:10:d5f847cc|ok:10:d5f847cc
err:unexpected EOF|err:unexpected EOF
ok:60:46b085d2|ok:60:46b085d2
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:9c3e1310|ok:60:9c3e1310
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:9dc8d6a|ok:60:9dc8d6a
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e2a98139|ok:60:e2a98139
ok:372:bc893748|ok:166:18efc291
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:85e6b5b7|ok:60:85e6b5b7
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:e40c8296|ok:60:e40c8296
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e6944e88|ok:60:e6944e88
err:unexpected EOF|err:unexpected EOF
ok:10:b0fd0a53|ok:10:b0fd0a53
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:3cfc2ade|ok:60:3cfc2ade
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:0:0|ok:85:6b10f76e
err:invalid distance|err:invalid distance
ok:10:6946d23b|ok:10:6946d23b
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:10:b95ce10b|ok:10:b95ce10b
err:unexpected EOF|err:unexpected EOF
ok:60:f86cd676|ok:60:f86cd676
ok:496:80971650|ok:185:d1f1849c
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:993db407|ok:60:993db407
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:6e032396|ok:60:6e032396
err:unexpected EOF|err:unexpected EOF
ok:10:8e6e2b8d|ok:10:8e6e2b8d
err:unexpected EOF|err:unexpected EOF
ok:60:914f79af|ok:60:914f79af
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:60:423ba4c6|ok:60:423ba4c6
ok:760:8b7c6deb|ok:602:dc4eb568
ok:10:be34d8f|ok:10:be34d8f
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:9d462b21|ok:10:9d462b21
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:5c752f67|ok:10:5c752f67
err:unexpected EOF|err:unexpected EOF
ok:60:135490a8|ok:60:135490a8
ok:376:623c44ce|ok:376:623c44ce
ok:10:3f39c6c4|ok:10:3f39c6c4
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:ed45841|ok:60:ed45841
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:bfcd742a|ok:60:bfcd742a
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:a4ad849d|ok:60:a4ad849d
err:invalid distance|err:invalid distance
ok:10:bfa5003f|ok:10:bfa5003f
err:unexpected EOF|err:unexpected EOF
ok:60:b5516718|ok:60:b5516718
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:bebc7466|ok:60:bebc7466
err:invalid distance|err:invalid distance
ok:10:6f0af22|ok:10:6f0af22
err:unexpected EOF|err:unexpected EOF
ok:60:85ed27c5|ok:60:85ed27c5
ok:372:19a6bc30|ok:243:3c7a0846
ok:10:1a4d9987|ok:10:1a4d9987
err:unexpected EOF|err:unexpected EOF
ok:60:162c4156|ok:60:162c4156
ok:372:ad645b9d|ok:372:ad645b9d
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:418d46ca|ok:60:418d46ca
err:invalid distance|err:invalid distance
ok:10:b9a124f1|ok:10:b9a124f1
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:0:0|ok:320:adc7574d
err:unexpected EOF|err:unexpected EOF
ok:60:aff79129|ok:60:aff79129
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d4ef956d|ok:60:d4ef956d
err:invalid distance|err:invalid distance
ok:10:73c32bcf|ok:10:73c32bcf
err:unexpected EOF|err:unexpected EOF
ok:60:842c56a4|ok:60:842c56a4
ok:372:236293ee|ok:88:66f8e49d
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:182722cc|err:offset is out of bounds
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1bf56765|ok:60:1bf56765
ok:116:5e4eedc9|ok:116:5e4eedc9
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:4124e457|ok:60:4124e457
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:caa28bbe|ok:60:caa28bbe
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:48f765a9|ok:60:48f765a9
ok:471:404deffa|ok:231:dde3dc4c
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:a43eadc7|ok:60:a43eadc7
err:unexpected EOF|err:unexpected EOF
ok:10:532ab5f0|ok:10:532ab5f0
err:unexpected EOF|err:unexpected EOF
ok:60:9035de9b|ok:60:9035de9b
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:ea27e8ad|ok:60:ea27e8ad
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:98727929|ok:10:98727929
err:unexpected EOF|err:unexpected EOF
ok:60:4edf096a|ok:60:4edf096a
err:unexpected EOF|err:unexpected EOF
ok:10:f1927828|ok:10:f1927828
err:unexpected EOF|err:unexpected EOF
ok:60:79825c07|ok:60:79825c07
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:7e9facc0|ok:60:7e9facc0
ok:372:41906301|ok:43:b8e880cc
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:f15ddee8|ok:60:f15ddee8
ok:372:ad223bb7|ok:226:c5ddc
ok:10:51281d7e|ok:10:51281d7e
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:9fdc9839|ok:60:9fdc9839
err:invalid distance|err:invalid distance
ok:0:0|ok:571:68f4f788
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:578c85ff|ok:10:578c85ff
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:c4e17b7b|ok:60:c4e17b7b
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:3febfbb|ok:60:3febfbb
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:bc674cd6|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:69d868f3|ok:60:69d868f3
err:invalid distance|err:invalid distance
ok:10:a36f742|ok:6:d13b2511
err:unexpected EOF|err:unexpected EOF
ok:60:352f04f|ok:60:352f04f
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:27365510|ok:60:27365510
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:4514b934|ok:60:4514b934
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:c2915976|ok:60:c2915976
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:f6612da8|ok:60:f6612da8
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:93a6dc22|ok:60:93a6dc22
ok:372:37b323b7|ok:372:37b323b7
ok:10:e4edbc2|ok:10:e4edbc2
err:unexpected EOF|err:unexpected EOF
ok:60:c6c55e63|ok:60:c6c55e63
err:invalid length/literal|err:invalid length/literal
ok:10:297c565|ok:10:297c565
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:10:d4813a5f|ok:10:d4813a5f
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid length/literal|err:invalid length/literal
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:bb5f5c5a|ok:10:bb5f5c5a
err:unexpected EOF|err:unexpected EOF
ok:60:13a8c984|ok:60:13a8c984
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:a0221504|ok:60:a0221504
ok:498:d5472c53|ok:49:a6a480b8
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
ok:60:30b10a56|ok:60:30b10a56
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:c50abca9|ok:60:c50abca9
ok:372:81ae5b13|ok:372:81ae5b13
ok:10:f1291a3c|ok:10:f1291a3c
err:unexpected EOF|err:unexpected EOF
ok:60:1b791c3d|err:offset is out of bounds
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:f5e98406|ok:60:f5e98406
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:fa9f53a4|ok:60:fa9f53a4
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:a4289c18|ok:60:a4289c18
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:2221ce5e|ok:60:2221ce5e
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:b815389c|ok:60:b815389c
ok:5:93b93ea4|ok:5:93b93ea4
ok:10:dfd9c5db|ok:10:dfd9c5db
err:unexpected EOF|err:unexpected EOF
ok:60:33dfb0a3|ok:60:33dfb0a3
err:unexpected EOF|err:unexpected EOF
ok:10:7b0bff98|ok:10:7b0bff98
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:60bd7a0|ok:150:d95967a9
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:5db9760b|ok:60:5db9760b
err:invalid distance|err:invalid distance
ok:10:2ca9abca|ok:10:2ca9abca
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:10:8dd6c782|ok:10:8dd6c782
err:unexpected EOF|err:unexpected EOF
ok:60:9d16a305|ok:60:9d16a305
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:4fabd494|ok:60:4fabd494
ok:372:755ffc5b|ok:83:15e0055f
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:ca21b2b4|ok:60:ca21b2b4
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:3b311768|ok:10:3b311768
err:unexpected EOF|err:unexpected EOF
ok:60:c145f6cd|ok:60:c145f6cd
ok:372:c5cd1d91|ok:219:661a186e
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:59551e9f|ok:60:59551e9f
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:ec76e58b|ok:60:ec76e58b
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:552806b1|ok:60:552806b1
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:7422ac0|ok:60:7422ac0
err:invalid distance|err:invalid distance
ok:10:3a0db750|ok:10:3a0db750
err:unexpected EOF|err:unexpected EOF
ok:60:8173b3a3|ok:60:8173b3a3
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:bb3e6717|ok:60:bb3e6717
ok:372:838880a4|ok:133:bae24169
ok:10:1be0f93f|ok:10:1be0f93f
err:unexpected EOF|err:unexpected EOF
ok:60:9749aec1|ok:60:9749aec1
ok:372:9a2d4ea1|ok:372:9a2d4ea1
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1179eb4c|ok:60:1179eb4c
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1e41de10|ok:60:1e41de10
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:9537ab42|ok:60:9537ab42
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:498:aaafc435|ok:498:aaafc435
ok:10:832eacbe|ok:10:832eacbe
err:unexpected EOF|err:unexpected EOF
ok:60:4baf788b|ok:60:4baf788b
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:10:80317c73|ok:10:80317c73
err:unexpected EOF|err:unexpected EOF
ok:60:cd4b1fb7|ok:60:cd4b1fb7
ok:381:7b528d1|ok:298:f5c26d67
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:c906c557|ok:60:c906c557
err:invalid distance|err:invalid distance
ok:10:eb41a3a1|ok:10:eb41a3a1
err:unexpected EOF|err:unexpected EOF
ok:60:86207938|ok:60:86207938
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:372:2f2beb81|ok:288:d8cc7e37
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:80ed7635|ok:10:80ed7635
err:unexpected EOF|err:unexpected EOF
ok:60:e8d944ee|ok:60:e8d944ee
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:240f22a7|ok:60:240f22a7
err:invalid distance|err:invalid distance
ok:10:b7a98e64|ok:10:b7a98e64
err:unexpected EOF|err:unexpected EOF
ok:60:d89325db|ok:60:d89325db
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:a65a6ce2|ok:10:a65a6ce2
err:unexpected EOF|err:unexpected EOF
ok:60:2a7a9c67|ok:60:2a7a9c67
ok:372:2147abf7|ok:372:2147abf7
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:51ad8d84|ok:60:51ad8d84
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:52b48e6d|ok:60:52b48e6d
err:invalid distance|err:invalid distance
ok:10:8779405d|ok:10:8779405d
err:unexpected EOF|err:unexpected EOF
ok:60:c9d667f3|ok:60:c9d667f3
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:df38118e|ok:60:df38118e
ok:498:a5eb856|ok:256:d246e960
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:532f297d|ok:60:532f297d
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:fcf234fb|ok:60:fcf234fb
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:d8384dcb|ok:60:d8384dcb
err:invalid distance|err:invalid distance
ok:10:2d656fc7|ok:10:2d656fc7
err:unexpected EOF|err:unexpected EOF
ok:60:ddd96616|err:offset is out of bounds
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1848d681|ok:60:1848d681
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:f55f8888|ok:60:f55f8888
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:cfb2beaf|ok:60:cfb2beaf
ok:372:a5ddb5d8|ok:372:a5ddb5d8
ok:10:c4ac39dc|ok:10:c4ac39dc
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:342:bbbc8ff4|ok:342:bbbc8ff4
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1c238d35|ok:60:1c238d35
ok:372:e616fdb1|ok:372:e616fdb1
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:f1cf24a4|ok:10:f1cf24a4
err:unexpected EOF|err:unexpected EOF
ok:60:30df6bf2|ok:60:30df6bf2
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:11a7c262|ok:60:11a7c262
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1d4fb12d|ok:60:1d4fb12d
err:invalid block type|err:invalid block type
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:29a162f6|ok:60:29a162f6
err:unexpected EOF|err:unexpected EOF
ok:10:9e49a329|ok:10:9e49a329
err:unexpected EOF|err:unexpected EOF
ok:60:a0c13cfd|ok:60:a0c13cfd
err:invalid distance|err:invalid distance
ok:10:5d76f486|ok:10:5d76f486
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:10:33dcde05|ok:10:33dcde05
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:48208002|ok:60:48208002
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:2cf8cbd9|ok:60:2cf8cbd9
err:invalid length/literal|err:invalid length/literal
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:372:9b9a5d90|ok:372:9b9a5d90
ok:352:6037ce48|ok:217:bd0b49b7
err:unexpected EOF|err:unexpected EOF
ok:60:5ae5b0c2|ok:60:5ae5b0c2
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:8972b01f|ok:10:8972b01f
err:unexpected EOF|err:unexpected EOF
ok:60:4e024fdc|ok:60:4e024fdc
err:invalid distance|err:invalid distance
ok:10:20ce1e4c|ok:10:20ce1e4c
err:unexpected EOF|err:unexpected EOF
ok:60:161a6a56|err:offset is out of bounds
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:994cce7c|ok:60:994cce7c
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:38624b2f|ok:60:38624b2f
err:invalid distance|err:invalid distance
ok:0:0|ok:329:3a0fa914
err:unexpected EOF|err:unexpected EOF
ok:60:cc658b4f|ok:60:cc658b4f
err:unexpected EOF|err:unexpected EOF
ok:10:ea8bef36|ok:10:ea8bef36
err:unexpected EOF|err:unexpected EOF
ok:60:4658152|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
ok:17:2bc53c81|ok:17:2bc53c81
err:unexpected EOF|err:unexpected EOF
ok:60:981c944c|ok:60:981c944c
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:ef6f03c8|ok:60:ef6f03c8
err:invalid distance|err:invalid distance
ok:10:5cfcff9a|ok:10:5cfcff9a
err:unexpected EOF|err:unexpected EOF
ok:60:e07bc6a0|ok:60:e07bc6a0
err:unexpected EOF|err:unexpected EOF
ok:10:a317ec22|ok:10:a317ec22
err:unexpected EOF|err:unexpected EOF
ok:60:6e20aa4f|ok:60:6e20aa4f
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:373:812e5e5|ok:373:812e5e5
ok:10:69ace039|ok:10:69ace039
err:unexpected EOF|err:unexpected EOF
ok:60:641bbf8a|ok:60:641bbf8a
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:e4014f7b|ok:60:e4014f7b
err:invalid distance|err:invalid distance
ok:10:141ae688|ok:10:141ae688
err:unexpected EOF|err:unexpected EOF
ok:60:89defd98|ok:60:89defd98
err:invalid distance|err:invalid distance
ok:10:db088f01|ok:10:db088f01
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:5:36178bae|ok:5:36178bae
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:383:490a6c18|ok:383:490a6c18
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d0907b4|ok:60:d0907b4
ok:372:28df1c32|ok:128:ca054a0a
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:cdf586c1|ok:60:cdf586c1
err:invalid distance|err:invalid distance
err:invalid block type|err:invalid block type
err:unexpected EOF|err:unexpected EOF
ok:60:da135b9a|ok:60:da135b9a
ok:372:e2a4b0b3|ok:35:79158052
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:e2167cb4|ok:60:e2167cb4
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:4a1c529|ok:60:4a1c529
ok:372:bc4cb3d4|ok:372:bc4cb3d4
ok:10:80f15d73|ok:10:80f15d73
err:unexpected EOF|err:unexpected EOF
ok:60:ae9e2736|ok:60:ae9e2736
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:a719e47a|err:offset is out of bounds
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:1a692306|ok:60:1a692306
err:unexpected EOF|err:unexpected EOF
ok:10:e8e79fa5|ok:10:e8e79fa5
err:unexpected EOF|err:unexpected EOF
ok:60:6a05825|ok:60:6a05825
err:invalid block type|err:invalid block type
ok:10:20ce1e4c|ok:10:20ce1e4c
err:unexpected EOF|err:unexpected EOF
ok:60:5b81da1a|ok:60:5b81da1a
ok:372:f7558d5a|ok:372:f7558d5a
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:225963b9|ok:60:225963b9
err:invalid length/literal|err:invalid length/literal
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:7f99b6a9|ok:60:7f99b6a9
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:a3215dfc|ok:60:a3215dfc
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:5d24218|ok:60:5d24218
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:ca5a9dbc|ok:60:ca5a9dbc
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:b820a275|ok:60:b820a275
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3e5a811d|ok:60:3e5a811d
ok:372:c828574d|ok:372:c828574d
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:46e041b6|err:offset is out of bounds
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:1790b358|ok:10:1790b358
err:unexpected EOF|err:unexpected EOF
ok:60:4caddfac|ok:60:4caddfac
err:invalid distance|err:invalid distance
ok:10:149a4d51|ok:10:149a4d51
err:unexpected EOF|err:unexpected EOF
ok:60:a34e2bb2|ok:60:a34e2bb2
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:3ece97d3|ok:60:3ece97d3
err:invalid distance|err:invalid distance
ok:10:783d4605|ok:10:783d4605
err:unexpected EOF|err:unexpected EOF
ok:60:3fc1a627|ok:60:3fc1a627
ok:372:82a46388|ok:372:82a46388
ok:10:abe713f|ok:10:abe713f
err:unexpected EOF|err:unexpected EOF
ok:60:1fea16a3|ok:60:1fea16a3
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:3a1cda8c|ok:60:3a1cda8c
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:4e598400|ok:60:4e598400
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:bff46a5b|ok:60:bff46a5b
ok:372:c3320b1f|ok:348:eee20bf0
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
ok:60:511c55df|ok:60:511c55df
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
ok:10:e2c525f4|ok:10:e2c525f4
err:unexpected EOF|err:unexpected EOF
ok:60:aac50751|ok:60:aac50751
err:unexpected EOF|err:unexpected EOF
ok:10:b9abe439|ok:10:b9abe439
err:unexpected EOF|err:unexpected EOF
ok:60:e67c2478|ok:60:e67c2478
err:invalid distance|err:invalid distance
ok:10:93823991|ok:10:93823991
err:unexpected EOF|err:unexpected EOF
ok:60:4514b16e|ok:60:4514b16e
ok:29:7a96005b|ok:29:7a96005b
ok:10:c8cfcaf8|ok:10:c8cfcaf8
err:unexpected EOF|err:unexpected EOF
ok:60:f7f015aa|ok:60:f7f015aa
err:invalid distance|err:invalid distance
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
err:invalid distance|err:invalid distance
err:unexpected EOF|err:unexpected EOF
err:unexpected EOF|err:unexpected EOF
ok:60:d3347e99|ok:60:d3347e99";

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

fn describe(r: uo_opc::Result<Vec<u8>>) -> String {
    match r {
        Ok(o) => format!("ok:{}:{:x}", o.len(), flate::crc32(&o)),
        Err(Error::Corrupt("stored block exceeds the declared size")) => "err:offset is out of bounds".into(),
        Err(Error::Corrupt(m)) => format!("err:{m}"),
        Err(e) => format!("err:{e:?}"),
    }
}

#[test]
fn mutated_streams_match_fflate() {
    let bases: Vec<Vec<u8>> = BASES.iter().map(|b| hex(b)).collect();
    let mut x: u32 = 0x9E37_79B9;
    let mut rnd = || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    let mut mismatches = Vec::new();
    let mut accepted_d1 = 0;
    for (i, want) in EXPECTED.lines().enumerate() {
        let mut d = bases[i % 4].clone();
        let m = 1 + rnd() % 3;
        for _ in 0..m {
            let p = (rnd() as usize) % d.len();
            d[p] = (rnd() & 255) as u8;
        }
        if rnd() % 5 == 0 {
            let n = (rnd() as usize) % d.len();
            d.truncate(n);
        }
        let decl = u64::from(rnd() % 700);
        for declared in [None, Some(decl)] {
            let full = flate::inflate_raw(&d, declared, 1 << 30);
            let check = flate::check_raw(&d, declared, 1 << 30);
            let same = match (&full, &check) {
                (Ok(o), Ok(c)) => c.len == o.len() as u64 && c.crc32 == flate::crc32(o) && c.stream_len >= c.len,
                (Err(a), Err(b)) => a == b,
                _ => false,
            };
            assert!(same, "case {i}, declared {declared:?}: inflate_raw {full:?}, check_raw {check:?}");
        }
        let got = format!("{}|{}", describe(flate::inflate_raw(&d, None, 1 << 30)), describe(flate::inflate_raw(&d, Some(decl), 1 << 30)));
        // D1: fflate's zeros for an empty stream with a declared size
        let d1 = format!("ok:0:0|ok:{decl}:{:x}", flate::crc32(&vec![0; decl as usize]));
        if d.is_empty() && decl > 0 && want == d1 && got == "ok:0:0|err:empty deflate stream with a declared size" {
            accepted_d1 += 1;
            continue;
        }
        if got != want {
            mismatches.push(format!("case {i}: got {got}, fflate {want}"));
        }
    }
    println!("accepted by name, D1 (empty stream with a declared size is Corrupt): {accepted_d1} cases");
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}
