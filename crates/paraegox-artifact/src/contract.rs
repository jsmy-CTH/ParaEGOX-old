use core::{fmt, num::NonZeroU64, str::FromStr};

use paraegox_kernel::digest::Digest32;
use sha2::{Digest as _, Sha256};

pub const PXAM_BYTES: usize = 206;
pub const PXAK_BYTES: usize = 72;
pub const PXAQ_BYTES: usize = 176;
pub const PXAA_BYTES: usize = 208;
pub const PXMU_BYTES: usize = 240;
pub const PXAV_BYTES: usize = 192;
pub const PXAW_BYTES: usize = 304;
pub const PXAX_BYTES: usize = 240;
pub const PXAZ_HEADER_BYTES: usize = 192;
pub const PXAY_HEADER_BYTES: usize = 64;
pub const PXOP_HEADER_BYTES: usize = 32;

pub const MAX_ARTIFACT_PAYLOAD_BYTES: usize = 64;
pub const MAX_ARTIFACT_OBJECTS: usize = 64;
pub const MAX_ARTIFACT_OPERATIONS: usize = 1024;
pub const MAX_ARTIFACT_QUARANTINE_BYTES: u64 = 540;
pub const MAX_PXAY_BODY_BYTES: usize =
    PXAY_HEADER_BYTES + MAX_ARTIFACT_OBJECTS * PXAV_BYTES + MAX_ARTIFACT_OPERATIONS * 1200;
pub const MAX_ARTIFACT_SNAPSHOT_BYTES: usize = PXAZ_HEADER_BYTES + MAX_PXAY_BODY_BYTES;
pub const ARTIFACT_DEFENSE_CEILING_BYTES: u64 = 8 * 1024 * 1024;

const PXAM_MAGIC: &[u8; 4] = b"PXAM";
const PXAK_MAGIC: &[u8; 4] = b"PXAK";
const PXAQ_MAGIC: &[u8; 4] = b"PXAQ";
const PXAA_MAGIC: &[u8; 4] = b"PXAA";
const PXMU_MAGIC: &[u8; 4] = b"PXMU";
const PXAV_MAGIC: &[u8; 4] = b"PXAV";
const PXAW_MAGIC: &[u8; 4] = b"PXAW";
const PXAX_MAGIC: &[u8; 4] = b"PXAX";
const PXAZ_MAGIC: &[u8; 4] = b"PXAZ";
const PXAY_MAGIC: &[u8; 4] = b"PXAY";
const PXOP_MAGIC: &[u8; 4] = b"PXOP";

const PROFILE: &[u8; 30] = b"developer-local-echo-prefix-v1";
const RUNTIME_KIND: &[u8; 21] = b"managed_model_data_v1";
const ADAPTER_ABI: &[u8; 26] = b"bounded-text-model-data-v1";
const TARGET_PROFILE: &[u8; 32] = b"developer-local-managed-model-v1";
const ENTRYPOINT: &[u8; 17] = b"literal-prefix-v1";

const PAYLOAD_DIGEST_DOMAIN: &[u8] = b"paraegox.artifact.payload.sha256.v1";
const MANIFEST_DIGEST_DOMAIN: &[u8] = b"paraegox.artifact.manifest.sha256.v1";
const REQUEST_DIGEST_DOMAIN: &[u8] = b"paraegox.artifact.materialization-request.sha256.v1";
const ADMISSION_DIGEST_DOMAIN: &[u8] = b"paraegox.artifact.materialization-admission.sha256.v1";
const MATERIALIZING_DIGEST_DOMAIN: &[u8] = b"paraegox.artifact.materializing.sha256.v1";
const OBJECT_TERMINAL_DIGEST_DOMAIN: &[u8] = b"paraegox.artifact.object-terminal.sha256.v1";
const OPERATION_TERMINAL_DIGEST_DOMAIN: &[u8] =
    b"paraegox.artifact.materialization-terminal.sha256.v1";
const RECEIPT_DIGEST_DOMAIN: &[u8] = b"paraegox.artifact.materialization-receipt.sha256.v1";
const SNAPSHOT_DIGEST_DOMAIN: &[u8] = b"paraegox.artifact.store-snapshot.sha256.v1";

const MAX_PROMPT_BYTES: u32 = 16_384;
const MAX_OUTPUT_BYTES: u32 = 32_768;
const MANIFEST_LEN_U32: u32 = PXAM_BYTES as u32;
const OBJECT_PUBLICATION_BLOCKED: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactContractError {
    InvalidLength,
    InvalidMagic,
    UnsupportedVersion,
    InvalidHeader,
    NonzeroReserved,
    InvalidLiteral,
    InvalidPayloadLength,
    DigestMismatch,
    ZeroStoreInstance,
    ZeroOperationId,
    ZeroConfigCommitment,
    ZeroSequence,
    InvalidState,
    InvalidReference,
    NonCanonicalEncoding,
    CrossFrameMismatch,
    DuplicateObject,
    DuplicateOperation,
    InvalidSnapshot,
    InvalidSuccessor,
    InvalidQuarantineFacts,
    CapacityExceeded,
    ArithmeticOverflow,
}

impl fmt::Display for ArtifactContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidLength => "artifact frame length is invalid",
            Self::InvalidMagic => "artifact frame magic is invalid",
            Self::UnsupportedVersion => "artifact frame version is unsupported",
            Self::InvalidHeader => "artifact frame header is invalid",
            Self::NonzeroReserved => "artifact reserved bytes must be zero",
            Self::InvalidLiteral => "artifact manifest literal is invalid",
            Self::InvalidPayloadLength => "artifact payload length is invalid",
            Self::DigestMismatch => "artifact digest does not match canonical bytes",
            Self::ZeroStoreInstance => "artifact store instance must be nonzero",
            Self::ZeroOperationId => "artifact operation id must be nonzero",
            Self::ZeroConfigCommitment => "artifact config commitment must be nonzero",
            Self::ZeroSequence => "artifact owner sequence must be nonzero",
            Self::InvalidState => "artifact state is invalid",
            Self::InvalidReference => "artifact text reference is invalid",
            Self::NonCanonicalEncoding => "artifact frame is not canonical",
            Self::CrossFrameMismatch => "artifact frame correlation is invalid",
            Self::DuplicateObject => "artifact object is duplicated",
            Self::DuplicateOperation => "artifact operation is duplicated",
            Self::InvalidSnapshot => "artifact snapshot invariant is invalid",
            Self::InvalidSuccessor => "artifact snapshot successor is invalid",
            Self::InvalidQuarantineFacts => "artifact quarantine facts are invalid",
            Self::CapacityExceeded => "artifact capacity is exceeded",
            Self::ArithmeticOverflow => "artifact capacity arithmetic overflowed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ArtifactContractError {}

macro_rules! nonzero_bytes {
    ($name:ident, $size:expr, $error:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name([u8; $size]);

        impl $name {
            pub const fn try_from_bytes(bytes: [u8; $size]) -> Result<Self, ArtifactContractError> {
                let mut index = 0;
                while index < bytes.len() {
                    if bytes[index] != 0 {
                        return Ok(Self(bytes));
                    }
                    index += 1;
                }
                Err(ArtifactContractError::$error)
            }

            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; $size] {
                &self.0
            }
        }
    };
}

nonzero_bytes!(ArtifactStoreInstanceV1, 32, ZeroStoreInstance);
nonzero_bytes!(ArtifactOperationIdV1, 16, ZeroOperationId);
nonzero_bytes!(ArtifactConfigCommitmentV1, 32, ZeroConfigCommitment);

impl ArtifactConfigCommitmentV1 {
    #[must_use]
    pub const fn digest(&self) -> Digest32 {
        Digest32::from_bytes(self.0)
    }
}

fn raw_sha256(domain: &[u8], parts: &[&[u8]]) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for part in parts {
        hasher.update(part);
    }
    Digest32::from_bytes(hasher.finalize().into())
}

fn validate_payload_bytes(payload: &[u8]) -> Result<(), ArtifactContractError> {
    if payload.is_empty()
        || payload.len() > MAX_ARTIFACT_PAYLOAD_BYTES
        || payload.last() != Some(&b' ')
        || !payload.iter().all(|byte| (0x20..=0x7e).contains(byte))
    {
        return Err(ArtifactContractError::InvalidPayloadLength);
    }
    Ok(())
}

fn read_array<const N: usize>(
    bytes: &[u8],
    start: usize,
) -> Result<[u8; N], ArtifactContractError> {
    let end = start
        .checked_add(N)
        .ok_or(ArtifactContractError::InvalidLength)?;
    let slice = bytes
        .get(start..end)
        .ok_or(ArtifactContractError::InvalidLength)?;
    <[u8; N]>::try_from(slice).map_err(|_| ArtifactContractError::InvalidLength)
}

fn read_u16(bytes: &[u8], start: usize) -> Result<u16, ArtifactContractError> {
    Ok(u16::from_be_bytes(read_array(bytes, start)?))
}

fn read_u32(bytes: &[u8], start: usize) -> Result<u32, ArtifactContractError> {
    Ok(u32::from_be_bytes(read_array(bytes, start)?))
}

fn read_u64(bytes: &[u8], start: usize) -> Result<u64, ArtifactContractError> {
    Ok(u64::from_be_bytes(read_array(bytes, start)?))
}

fn require_zero(bytes: &[u8]) -> Result<(), ArtifactContractError> {
    if bytes.iter().all(|byte| *byte == 0) {
        Ok(())
    } else {
        Err(ArtifactContractError::NonzeroReserved)
    }
}

fn optional_digest(bytes: [u8; 32]) -> Option<Digest32> {
    if bytes.iter().all(|byte| *byte == 0) {
        None
    } else {
        Some(Digest32::from_bytes(bytes))
    }
}

fn nonzero_digest(bytes: [u8; 32]) -> Result<Digest32, ArtifactContractError> {
    if bytes.iter().all(|byte| *byte == 0) {
        Err(ArtifactContractError::DigestMismatch)
    } else {
        Ok(Digest32::from_bytes(bytes))
    }
}

fn put_u16(bytes: &mut [u8], start: usize, value: u16) {
    bytes[start..start + 2].copy_from_slice(&value.to_be_bytes());
}

fn put_u32(bytes: &mut [u8], start: usize, value: u32) {
    bytes[start..start + 4].copy_from_slice(&value.to_be_bytes());
}

fn put_u64(bytes: &mut [u8], start: usize, value: u64) {
    bytes[start..start + 8].copy_from_slice(&value.to_be_bytes());
}

fn canonical_round_trip<const N: usize>(
    input: &[u8],
    encoded: &[u8; N],
) -> Result<(), ArtifactContractError> {
    if input == encoded {
        Ok(())
    } else {
        Err(ArtifactContractError::NonCanonicalEncoding)
    }
}

fn push_lower_hex(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
}

fn decode_lower_hex<const N: usize>(input: &str) -> Result<[u8; N], ArtifactContractError> {
    if input.len() != N * 2 || !input.is_ascii() {
        return Err(ArtifactContractError::InvalidReference);
    }
    let mut output = [0_u8; N];
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < N {
        let high = decode_nibble(bytes[index * 2])?;
        let low = decode_nibble(bytes[index * 2 + 1])?;
        output[index] = (high << 4) | low;
        index += 1;
    }
    Ok(output)
}

fn decode_nibble(byte: u8) -> Result<u8, ArtifactContractError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(ArtifactContractError::InvalidReference),
    }
}

/// Profile-only classification for an untrusted PXAM candidate.
///
/// This preclassification deliberately ignores every non-profile field. An
/// exact-size frame exposes both fixed profile fields, while any other length
/// remains indeterminate and belongs to the ordinary compatibility decoder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactManifestProfileClassificationV1 {
    Match,
    Mismatch,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactManifestV1 {
    payload_len: u64,
    payload_digest: Digest32,
}

impl ArtifactManifestV1 {
    #[must_use]
    pub fn classify_profile(bytes: &[u8]) -> ArtifactManifestProfileClassificationV1 {
        if bytes.len() != PXAM_BYTES {
            return ArtifactManifestProfileClassificationV1::Indeterminate;
        }
        let declared_profile_len = u16::from_be_bytes([bytes[12], bytes[13]]);
        if declared_profile_len != PROFILE.len() as u16
            || bytes.get(80..110) != Some(PROFILE.as_slice())
        {
            ArtifactManifestProfileClassificationV1::Mismatch
        } else {
            ArtifactManifestProfileClassificationV1::Match
        }
    }

    pub fn from_payload(payload: &[u8]) -> Result<Self, ArtifactContractError> {
        validate_payload_bytes(payload)?;
        let payload_digest = raw_sha256(PAYLOAD_DIGEST_DOMAIN, &[payload]);
        if payload_digest.as_bytes().iter().all(|byte| *byte == 0) {
            return Err(ArtifactContractError::DigestMismatch);
        }
        Ok(Self {
            payload_len: u64::try_from(payload.len())
                .map_err(|_| ArtifactContractError::InvalidPayloadLength)?,
            payload_digest,
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        if bytes.len() != PXAM_BYTES {
            return Err(ArtifactContractError::InvalidLength);
        }
        if bytes.get(0..4) != Some(PXAM_MAGIC.as_slice()) {
            return Err(ArtifactContractError::InvalidMagic);
        }
        if read_u16(bytes, 4)? != 1 {
            return Err(ArtifactContractError::UnsupportedVersion);
        }
        if read_u16(bytes, 6)? != 80
            || read_u32(bytes, 8)? != PXAM_BYTES as u32
            || read_u16(bytes, 12)? != PROFILE.len() as u16
            || read_u16(bytes, 14)? != RUNTIME_KIND.len() as u16
            || read_u16(bytes, 16)? != ADAPTER_ABI.len() as u16
            || read_u16(bytes, 18)? != TARGET_PROFILE.len() as u16
            || read_u16(bytes, 20)? != ENTRYPOINT.len() as u16
        {
            return Err(ArtifactContractError::InvalidHeader);
        }
        require_zero(&bytes[22..24])?;
        let payload_len = read_u64(bytes, 24)?;
        if !(1..=MAX_ARTIFACT_PAYLOAD_BYTES as u64).contains(&payload_len) {
            return Err(ArtifactContractError::InvalidPayloadLength);
        }
        if read_u32(bytes, 64)? != MAX_PROMPT_BYTES
            || read_u32(bytes, 68)? != MAX_OUTPUT_BYTES
            || read_u32(bytes, 72)? != 0
        {
            return Err(ArtifactContractError::InvalidHeader);
        }
        require_zero(&bytes[76..80])?;
        if bytes.get(80..110) != Some(PROFILE.as_slice())
            || bytes.get(110..131) != Some(RUNTIME_KIND.as_slice())
            || bytes.get(131..157) != Some(ADAPTER_ABI.as_slice())
            || bytes.get(157..189) != Some(TARGET_PROFILE.as_slice())
            || bytes.get(189..206) != Some(ENTRYPOINT.as_slice())
        {
            return Err(ArtifactContractError::InvalidLiteral);
        }
        let manifest = Self {
            payload_len,
            payload_digest: nonzero_digest(read_array(bytes, 32)?)?,
        };
        canonical_round_trip(bytes, &manifest.encode())?;
        Ok(manifest)
    }

    #[must_use]
    pub fn encode(&self) -> [u8; PXAM_BYTES] {
        let mut bytes = [0_u8; PXAM_BYTES];
        bytes[0..4].copy_from_slice(PXAM_MAGIC);
        put_u16(&mut bytes, 4, 1);
        put_u16(&mut bytes, 6, 80);
        put_u32(&mut bytes, 8, PXAM_BYTES as u32);
        put_u16(&mut bytes, 12, PROFILE.len() as u16);
        put_u16(&mut bytes, 14, RUNTIME_KIND.len() as u16);
        put_u16(&mut bytes, 16, ADAPTER_ABI.len() as u16);
        put_u16(&mut bytes, 18, TARGET_PROFILE.len() as u16);
        put_u16(&mut bytes, 20, ENTRYPOINT.len() as u16);
        put_u64(&mut bytes, 24, self.payload_len);
        bytes[32..64].copy_from_slice(self.payload_digest.as_bytes());
        put_u32(&mut bytes, 64, MAX_PROMPT_BYTES);
        put_u32(&mut bytes, 68, MAX_OUTPUT_BYTES);
        bytes[80..110].copy_from_slice(PROFILE);
        bytes[110..131].copy_from_slice(RUNTIME_KIND);
        bytes[131..157].copy_from_slice(ADAPTER_ABI);
        bytes[157..189].copy_from_slice(TARGET_PROFILE);
        bytes[189..206].copy_from_slice(ENTRYPOINT);
        bytes
    }

    #[must_use]
    pub const fn payload_len(&self) -> u64 {
        self.payload_len
    }

    #[must_use]
    pub const fn payload_digest(&self) -> Digest32 {
        self.payload_digest
    }

    #[must_use]
    pub fn manifest_digest(&self) -> Digest32 {
        let bytes = self.encode();
        raw_sha256(MANIFEST_DIGEST_DOMAIN, &[&bytes])
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ArtifactObjectRefV1 {
    payload_digest: Digest32,
    manifest_digest: Digest32,
}

impl ArtifactObjectRefV1 {
    pub fn try_new(
        payload_digest: Digest32,
        manifest_digest: Digest32,
    ) -> Result<Self, ArtifactContractError> {
        if payload_digest.as_bytes().iter().all(|byte| *byte == 0)
            || manifest_digest.as_bytes().iter().all(|byte| *byte == 0)
        {
            return Err(ArtifactContractError::DigestMismatch);
        }
        Ok(Self {
            payload_digest,
            manifest_digest,
        })
    }

    pub fn from_manifest(manifest: &ArtifactManifestV1) -> Result<Self, ArtifactContractError> {
        Self::try_new(manifest.payload_digest(), manifest.manifest_digest())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        if bytes.len() != PXAK_BYTES {
            return Err(ArtifactContractError::InvalidLength);
        }
        if bytes.get(0..4) != Some(PXAK_MAGIC.as_slice()) {
            return Err(ArtifactContractError::InvalidMagic);
        }
        if read_u16(bytes, 4)? != 1 {
            return Err(ArtifactContractError::UnsupportedVersion);
        }
        if read_u16(bytes, 6)? != PXAK_BYTES as u16 {
            return Err(ArtifactContractError::InvalidHeader);
        }
        let reference = Self::try_new(
            nonzero_digest(read_array(bytes, 8)?)?,
            nonzero_digest(read_array(bytes, 40)?)?,
        )?;
        canonical_round_trip(bytes, &reference.encode())?;
        Ok(reference)
    }

    #[must_use]
    pub fn encode(&self) -> [u8; PXAK_BYTES] {
        let mut bytes = [0_u8; PXAK_BYTES];
        bytes[0..4].copy_from_slice(PXAK_MAGIC);
        put_u16(&mut bytes, 4, 1);
        put_u16(&mut bytes, 6, PXAK_BYTES as u16);
        bytes[8..40].copy_from_slice(self.payload_digest.as_bytes());
        bytes[40..72].copy_from_slice(self.manifest_digest.as_bytes());
        bytes
    }

    #[must_use]
    pub const fn payload_digest(&self) -> Digest32 {
        self.payload_digest
    }

    #[must_use]
    pub const fn manifest_digest(&self) -> Digest32 {
        self.manifest_digest
    }
}

impl fmt::Display for ArtifactObjectRefV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut output = String::with_capacity(7 + 64 + 1 + 64);
        output.push_str("sha256:");
        push_lower_hex(&mut output, self.payload_digest.as_bytes());
        output.push(':');
        push_lower_hex(&mut output, self.manifest_digest.as_bytes());
        formatter.write_str(&output)
    }
}

impl FromStr for ArtifactObjectRefV1 {
    type Err = ArtifactContractError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut fields = input.split(':');
        if fields.next() != Some("sha256") {
            return Err(ArtifactContractError::InvalidReference);
        }
        let payload = fields
            .next()
            .ok_or(ArtifactContractError::InvalidReference)?;
        let manifest = fields
            .next()
            .ok_or(ArtifactContractError::InvalidReference)?;
        if fields.next().is_some() {
            return Err(ArtifactContractError::InvalidReference);
        }
        let value = Self::try_new(
            nonzero_digest(decode_lower_hex(payload)?)?,
            nonzero_digest(decode_lower_hex(manifest)?)?,
        )?;
        if value.to_string() != input {
            return Err(ArtifactContractError::InvalidReference);
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedArtifactPairV1 {
    manifest: ArtifactManifestV1,
    manifest_bytes: [u8; PXAM_BYTES],
    payload: Box<[u8]>,
    object_ref: ArtifactObjectRefV1,
}

impl VerifiedArtifactPairV1 {
    pub fn verify(manifest_bytes: &[u8], payload: &[u8]) -> Result<Self, ArtifactContractError> {
        validate_payload_bytes(payload)?;
        let manifest = ArtifactManifestV1::decode(manifest_bytes)?;
        if usize::try_from(manifest.payload_len()).ok() != Some(payload.len())
            || manifest.payload_digest() != raw_sha256(PAYLOAD_DIGEST_DOMAIN, &[payload])
        {
            return Err(ArtifactContractError::DigestMismatch);
        }
        let manifest_bytes = <[u8; PXAM_BYTES]>::try_from(manifest_bytes)
            .map_err(|_| ArtifactContractError::InvalidLength)?;
        let object_ref = ArtifactObjectRefV1::from_manifest(&manifest)?;
        Ok(Self {
            manifest,
            manifest_bytes,
            payload: payload.into(),
            object_ref,
        })
    }

    pub fn from_payload(payload: &[u8]) -> Result<Self, ArtifactContractError> {
        let manifest = ArtifactManifestV1::from_payload(payload)?;
        Self::verify(&manifest.encode(), payload)
    }

    #[must_use]
    pub const fn manifest(&self) -> &ArtifactManifestV1 {
        &self.manifest
    }

    #[must_use]
    pub const fn manifest_bytes(&self) -> &[u8; PXAM_BYTES] {
        &self.manifest_bytes
    }

    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }
}

fn check_frame_prefix(
    bytes: &[u8],
    expected_len: usize,
    magic: &[u8; 4],
    action: u8,
    state: u8,
) -> Result<(), ArtifactContractError> {
    if bytes.len() != expected_len {
        return Err(ArtifactContractError::InvalidLength);
    }
    if bytes.get(0..4) != Some(magic.as_slice()) {
        return Err(ArtifactContractError::InvalidMagic);
    }
    if read_u16(bytes, 4)? != 1 {
        return Err(ArtifactContractError::UnsupportedVersion);
    }
    if bytes[6] != action || bytes[7] != state {
        return Err(ArtifactContractError::InvalidState);
    }
    if read_u16(bytes, 8)? != expected_len as u16 || read_u32(bytes, 12)? != expected_len as u32 {
        return Err(ArtifactContractError::InvalidHeader);
    }
    require_zero(&bytes[10..12])
}

fn put_frame_prefix<const N: usize>(bytes: &mut [u8; N], magic: &[u8; 4], action: u8, state: u8) {
    bytes[0..4].copy_from_slice(magic);
    put_u16(bytes, 4, 1);
    bytes[6] = action;
    bytes[7] = state;
    put_u16(bytes, 8, N as u16);
    put_u32(bytes, 12, N as u32);
}

fn require_digest(
    actual: Digest32,
    domain: &[u8],
    bytes: &[u8],
) -> Result<(), ArtifactContractError> {
    if actual == raw_sha256(domain, &[bytes]) {
        Ok(())
    } else {
        Err(ArtifactContractError::DigestMismatch)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationRequestV1 {
    operation_id: ArtifactOperationIdV1,
    config_commitment: ArtifactConfigCommitmentV1,
    object_ref: ArtifactObjectRefV1,
    request_digest: Digest32,
}

impl MaterializationRequestV1 {
    #[must_use]
    pub fn new(
        operation_id: ArtifactOperationIdV1,
        config_commitment: ArtifactConfigCommitmentV1,
        object_ref: ArtifactObjectRefV1,
    ) -> Self {
        let mut value = Self {
            operation_id,
            config_commitment,
            object_ref,
            request_digest: Digest32::from_bytes([0; 32]),
        };
        let bytes = value.encode_prefix();
        value.request_digest = raw_sha256(REQUEST_DIGEST_DOMAIN, &[&bytes]);
        value
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        check_frame_prefix(bytes, PXAQ_BYTES, PXAQ_MAGIC, b'M', 0)?;
        if read_u32(bytes, 136)? != 0 {
            return Err(ArtifactContractError::InvalidHeader);
        }
        require_zero(&bytes[140..144])?;
        let value = Self {
            operation_id: ArtifactOperationIdV1::try_from_bytes(read_array(bytes, 16)?)?,
            config_commitment: ArtifactConfigCommitmentV1::try_from_bytes(read_array(bytes, 32)?)?,
            object_ref: ArtifactObjectRefV1::decode(&bytes[64..136])?,
            request_digest: Digest32::from_bytes(read_array(bytes, 144)?),
        };
        require_digest(value.request_digest, REQUEST_DIGEST_DOMAIN, &bytes[..144])?;
        canonical_round_trip(bytes, &value.encode())?;
        Ok(value)
    }

    fn encode_prefix(&self) -> [u8; 144] {
        let mut bytes = [0_u8; 144];
        put_frame_prefix(&mut bytes, PXAQ_MAGIC, b'M', 0);
        put_u16(&mut bytes, 8, PXAQ_BYTES as u16);
        put_u32(&mut bytes, 12, PXAQ_BYTES as u32);
        bytes[16..32].copy_from_slice(self.operation_id.as_bytes());
        bytes[32..64].copy_from_slice(self.config_commitment.as_bytes());
        bytes[64..136].copy_from_slice(&self.object_ref.encode());
        bytes
    }

    #[must_use]
    pub fn encode(&self) -> [u8; PXAQ_BYTES] {
        let mut bytes = [0_u8; PXAQ_BYTES];
        bytes[..144].copy_from_slice(&self.encode_prefix());
        bytes[144..176].copy_from_slice(self.request_digest.as_bytes());
        bytes
    }

    #[must_use]
    pub const fn operation_id(&self) -> ArtifactOperationIdV1 {
        self.operation_id
    }

    #[must_use]
    pub const fn config_commitment(&self) -> ArtifactConfigCommitmentV1 {
        self.config_commitment
    }

    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }

    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationAdmissionV1 {
    store_instance: ArtifactStoreInstanceV1,
    operation_sequence: NonZeroU64,
    operation_id: ArtifactOperationIdV1,
    request_digest: Digest32,
    object_ref: ArtifactObjectRefV1,
    admission_digest: Digest32,
}

impl MaterializationAdmissionV1 {
    #[must_use]
    pub fn new(
        store_instance: ArtifactStoreInstanceV1,
        operation_sequence: NonZeroU64,
        request: &MaterializationRequestV1,
    ) -> Self {
        let mut value = Self {
            store_instance,
            operation_sequence,
            operation_id: request.operation_id(),
            request_digest: request.request_digest(),
            object_ref: request.object_ref(),
            admission_digest: Digest32::from_bytes([0; 32]),
        };
        let bytes = value.encode_prefix();
        value.admission_digest = raw_sha256(ADMISSION_DIGEST_DOMAIN, &[&bytes]);
        value
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        check_frame_prefix(bytes, PXAA_BYTES, PXAA_MAGIC, b'M', b'A')?;
        let value = Self {
            store_instance: ArtifactStoreInstanceV1::try_from_bytes(read_array(bytes, 16)?)?,
            operation_sequence: NonZeroU64::new(read_u64(bytes, 48)?)
                .ok_or(ArtifactContractError::ZeroSequence)?,
            operation_id: ArtifactOperationIdV1::try_from_bytes(read_array(bytes, 56)?)?,
            request_digest: nonzero_digest(read_array(bytes, 72)?)?,
            object_ref: ArtifactObjectRefV1::decode(&bytes[104..176])?,
            admission_digest: Digest32::from_bytes(read_array(bytes, 176)?),
        };
        require_digest(
            value.admission_digest,
            ADMISSION_DIGEST_DOMAIN,
            &bytes[..176],
        )?;
        canonical_round_trip(bytes, &value.encode())?;
        Ok(value)
    }

    fn encode_prefix(&self) -> [u8; 176] {
        let mut bytes = [0_u8; 176];
        put_frame_prefix(&mut bytes, PXAA_MAGIC, b'M', b'A');
        put_u16(&mut bytes, 8, PXAA_BYTES as u16);
        put_u32(&mut bytes, 12, PXAA_BYTES as u32);
        bytes[16..48].copy_from_slice(self.store_instance.as_bytes());
        put_u64(&mut bytes, 48, self.operation_sequence.get());
        bytes[56..72].copy_from_slice(self.operation_id.as_bytes());
        bytes[72..104].copy_from_slice(self.request_digest.as_bytes());
        bytes[104..176].copy_from_slice(&self.object_ref.encode());
        bytes
    }

    #[must_use]
    pub fn encode(&self) -> [u8; PXAA_BYTES] {
        let mut bytes = [0_u8; PXAA_BYTES];
        bytes[..176].copy_from_slice(&self.encode_prefix());
        bytes[176..208].copy_from_slice(self.admission_digest.as_bytes());
        bytes
    }

    #[must_use]
    pub const fn store_instance(&self) -> ArtifactStoreInstanceV1 {
        self.store_instance
    }
    #[must_use]
    pub const fn operation_sequence(&self) -> NonZeroU64 {
        self.operation_sequence
    }
    #[must_use]
    pub const fn operation_id(&self) -> ArtifactOperationIdV1 {
        self.operation_id
    }
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }
    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }
    #[must_use]
    pub const fn admission_digest(&self) -> Digest32 {
        self.admission_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializingRecordV1 {
    store_instance: ArtifactStoreInstanceV1,
    operation_sequence: NonZeroU64,
    operation_id: ArtifactOperationIdV1,
    request_digest: Digest32,
    admission_digest: Digest32,
    object_ref: ArtifactObjectRefV1,
    materializing_digest: Digest32,
}

impl MaterializingRecordV1 {
    #[must_use]
    pub fn new(admission: &MaterializationAdmissionV1) -> Self {
        let mut value = Self {
            store_instance: admission.store_instance(),
            operation_sequence: admission.operation_sequence(),
            operation_id: admission.operation_id(),
            request_digest: admission.request_digest(),
            admission_digest: admission.admission_digest(),
            object_ref: admission.object_ref(),
            materializing_digest: Digest32::from_bytes([0; 32]),
        };
        let bytes = value.encode_prefix();
        value.materializing_digest = raw_sha256(MATERIALIZING_DIGEST_DOMAIN, &[&bytes]);
        value
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        check_frame_prefix(bytes, PXMU_BYTES, PXMU_MAGIC, b'M', b'P')?;
        let value = Self {
            store_instance: ArtifactStoreInstanceV1::try_from_bytes(read_array(bytes, 16)?)?,
            operation_sequence: NonZeroU64::new(read_u64(bytes, 48)?)
                .ok_or(ArtifactContractError::ZeroSequence)?,
            operation_id: ArtifactOperationIdV1::try_from_bytes(read_array(bytes, 56)?)?,
            request_digest: nonzero_digest(read_array(bytes, 72)?)?,
            admission_digest: nonzero_digest(read_array(bytes, 104)?)?,
            object_ref: ArtifactObjectRefV1::decode(&bytes[136..208])?,
            materializing_digest: Digest32::from_bytes(read_array(bytes, 208)?),
        };
        require_digest(
            value.materializing_digest,
            MATERIALIZING_DIGEST_DOMAIN,
            &bytes[..208],
        )?;
        canonical_round_trip(bytes, &value.encode())?;
        Ok(value)
    }

    fn encode_prefix(&self) -> [u8; 208] {
        let mut bytes = [0_u8; 208];
        put_frame_prefix(&mut bytes, PXMU_MAGIC, b'M', b'P');
        put_u16(&mut bytes, 8, PXMU_BYTES as u16);
        put_u32(&mut bytes, 12, PXMU_BYTES as u32);
        bytes[16..48].copy_from_slice(self.store_instance.as_bytes());
        put_u64(&mut bytes, 48, self.operation_sequence.get());
        bytes[56..72].copy_from_slice(self.operation_id.as_bytes());
        bytes[72..104].copy_from_slice(self.request_digest.as_bytes());
        bytes[104..136].copy_from_slice(self.admission_digest.as_bytes());
        bytes[136..208].copy_from_slice(&self.object_ref.encode());
        bytes
    }

    #[must_use]
    pub fn encode(&self) -> [u8; PXMU_BYTES] {
        let mut bytes = [0_u8; PXMU_BYTES];
        bytes[..208].copy_from_slice(&self.encode_prefix());
        bytes[208..240].copy_from_slice(self.materializing_digest.as_bytes());
        bytes
    }

    #[must_use]
    pub const fn store_instance(&self) -> ArtifactStoreInstanceV1 {
        self.store_instance
    }
    #[must_use]
    pub const fn operation_sequence(&self) -> NonZeroU64 {
        self.operation_sequence
    }
    #[must_use]
    pub const fn operation_id(&self) -> ArtifactOperationIdV1 {
        self.operation_id
    }
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }
    #[must_use]
    pub const fn admission_digest(&self) -> Digest32 {
        self.admission_digest
    }
    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }
    #[must_use]
    pub const fn materializing_digest(&self) -> Digest32 {
        self.materializing_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactObjectRecordV1 {
    store_instance: ArtifactStoreInstanceV1,
    object_sequence: NonZeroU64,
    object_ref: ArtifactObjectRefV1,
    payload_len: u64,
    object_terminal_digest: Digest32,
}

impl ArtifactObjectRecordV1 {
    #[must_use]
    pub fn new(
        store_instance: ArtifactStoreInstanceV1,
        object_sequence: NonZeroU64,
        pair: &VerifiedArtifactPairV1,
    ) -> Self {
        let mut value = Self {
            store_instance,
            object_sequence,
            object_ref: pair.object_ref(),
            payload_len: pair.manifest().payload_len(),
            object_terminal_digest: Digest32::from_bytes([0; 32]),
        };
        let bytes = value.encode_prefix();
        value.object_terminal_digest = raw_sha256(OBJECT_TERMINAL_DIGEST_DOMAIN, &[&bytes]);
        value
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        check_frame_prefix(bytes, PXAV_BYTES, PXAV_MAGIC, b'M', b'R')?;
        let payload_len = read_u64(bytes, 128)?;
        if !(1..=MAX_ARTIFACT_PAYLOAD_BYTES as u64).contains(&payload_len)
            || read_u32(bytes, 136)? != MANIFEST_LEN_U32
        {
            return Err(ArtifactContractError::InvalidPayloadLength);
        }
        require_zero(&bytes[140..160])?;
        let value = Self {
            store_instance: ArtifactStoreInstanceV1::try_from_bytes(read_array(bytes, 16)?)?,
            object_sequence: NonZeroU64::new(read_u64(bytes, 48)?)
                .ok_or(ArtifactContractError::ZeroSequence)?,
            object_ref: ArtifactObjectRefV1::decode(&bytes[56..128])?,
            payload_len,
            object_terminal_digest: Digest32::from_bytes(read_array(bytes, 160)?),
        };
        require_digest(
            value.object_terminal_digest,
            OBJECT_TERMINAL_DIGEST_DOMAIN,
            &bytes[..160],
        )?;
        canonical_round_trip(bytes, &value.encode())?;
        Ok(value)
    }

    fn encode_prefix(&self) -> [u8; 160] {
        let mut bytes = [0_u8; 160];
        put_frame_prefix(&mut bytes, PXAV_MAGIC, b'M', b'R');
        put_u16(&mut bytes, 8, PXAV_BYTES as u16);
        put_u32(&mut bytes, 12, PXAV_BYTES as u32);
        bytes[16..48].copy_from_slice(self.store_instance.as_bytes());
        put_u64(&mut bytes, 48, self.object_sequence.get());
        bytes[56..128].copy_from_slice(&self.object_ref.encode());
        put_u64(&mut bytes, 128, self.payload_len);
        put_u32(&mut bytes, 136, MANIFEST_LEN_U32);
        bytes
    }

    #[must_use]
    pub fn encode(&self) -> [u8; PXAV_BYTES] {
        let mut bytes = [0_u8; PXAV_BYTES];
        bytes[..160].copy_from_slice(&self.encode_prefix());
        bytes[160..192].copy_from_slice(self.object_terminal_digest.as_bytes());
        bytes
    }

    #[must_use]
    pub const fn store_instance(&self) -> ArtifactStoreInstanceV1 {
        self.store_instance
    }
    #[must_use]
    pub const fn object_sequence(&self) -> NonZeroU64 {
        self.object_sequence
    }
    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }
    #[must_use]
    pub const fn payload_len(&self) -> u64 {
        self.payload_len
    }
    #[must_use]
    pub const fn object_terminal_digest(&self) -> Digest32 {
        self.object_terminal_digest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaterializationTerminalStateV1 {
    Materialized,
    AlreadyMaterialized,
    Failed,
    Uncertain,
}

impl MaterializationTerminalStateV1 {
    const fn byte(self) -> u8 {
        match self {
            Self::Materialized => b'M',
            Self::AlreadyMaterialized => b'E',
            Self::Failed => b'F',
            Self::Uncertain => b'U',
        }
    }

    fn from_byte(byte: u8) -> Result<Self, ArtifactContractError> {
        match byte {
            b'M' => Ok(Self::Materialized),
            b'E' => Ok(Self::AlreadyMaterialized),
            b'F' => Ok(Self::Failed),
            b'U' => Ok(Self::Uncertain),
            _ => Err(ArtifactContractError::InvalidState),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationTerminalV1 {
    store_instance: ArtifactStoreInstanceV1,
    operation_sequence: NonZeroU64,
    operation_id: ArtifactOperationIdV1,
    request_digest: Digest32,
    admission_digest: Digest32,
    materializing_digest: Option<Digest32>,
    object_terminal_digest: Option<Digest32>,
    object_ref: ArtifactObjectRefV1,
    state: MaterializationTerminalStateV1,
    terminal_digest: Digest32,
}

impl MaterializationTerminalV1 {
    pub fn new(
        admission: &MaterializationAdmissionV1,
        materializing: Option<&MaterializingRecordV1>,
        object: Option<&ArtifactObjectRecordV1>,
        state: MaterializationTerminalStateV1,
    ) -> Result<Self, ArtifactContractError> {
        if materializing.is_some_and(|record| {
            record.store_instance() != admission.store_instance()
                || record.operation_sequence() != admission.operation_sequence()
                || record.operation_id() != admission.operation_id()
                || record.request_digest() != admission.request_digest()
                || record.admission_digest() != admission.admission_digest()
                || record.object_ref() != admission.object_ref()
        }) || object.is_some_and(|record| {
            record.store_instance() != admission.store_instance()
                || record.object_ref() != admission.object_ref()
        }) {
            return Err(ArtifactContractError::CrossFrameMismatch);
        }
        let materializing_digest = materializing.map(MaterializingRecordV1::materializing_digest);
        let object_terminal_digest = object.map(ArtifactObjectRecordV1::object_terminal_digest);
        match state {
            MaterializationTerminalStateV1::Materialized
            | MaterializationTerminalStateV1::AlreadyMaterialized => {
                if materializing_digest.is_none() || object_terminal_digest.is_none() {
                    return Err(ArtifactContractError::InvalidState);
                }
            }
            MaterializationTerminalStateV1::Failed => {
                if object_terminal_digest.is_some() {
                    return Err(ArtifactContractError::InvalidState);
                }
            }
            MaterializationTerminalStateV1::Uncertain => {
                if materializing_digest.is_none() || object_terminal_digest.is_some() {
                    return Err(ArtifactContractError::InvalidState);
                }
            }
        }
        let mut value = Self {
            store_instance: admission.store_instance(),
            operation_sequence: admission.operation_sequence(),
            operation_id: admission.operation_id(),
            request_digest: admission.request_digest(),
            admission_digest: admission.admission_digest(),
            materializing_digest,
            object_terminal_digest,
            object_ref: admission.object_ref(),
            state,
            terminal_digest: Digest32::from_bytes([0; 32]),
        };
        let bytes = value.encode_prefix();
        value.terminal_digest = raw_sha256(OPERATION_TERMINAL_DIGEST_DOMAIN, &[&bytes]);
        Ok(value)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        if bytes.len() != PXAW_BYTES {
            return Err(ArtifactContractError::InvalidLength);
        }
        let state = MaterializationTerminalStateV1::from_byte(bytes[7])?;
        check_frame_prefix(bytes, PXAW_BYTES, PXAW_MAGIC, b'M', state.byte())?;
        let value = Self {
            store_instance: ArtifactStoreInstanceV1::try_from_bytes(read_array(bytes, 16)?)?,
            operation_sequence: NonZeroU64::new(read_u64(bytes, 48)?)
                .ok_or(ArtifactContractError::ZeroSequence)?,
            operation_id: ArtifactOperationIdV1::try_from_bytes(read_array(bytes, 56)?)?,
            request_digest: nonzero_digest(read_array(bytes, 72)?)?,
            admission_digest: nonzero_digest(read_array(bytes, 104)?)?,
            materializing_digest: optional_digest(read_array(bytes, 136)?),
            object_terminal_digest: optional_digest(read_array(bytes, 168)?),
            object_ref: ArtifactObjectRefV1::decode(&bytes[200..272])?,
            state,
            terminal_digest: Digest32::from_bytes(read_array(bytes, 272)?),
        };
        match state {
            MaterializationTerminalStateV1::Materialized
            | MaterializationTerminalStateV1::AlreadyMaterialized => {
                if value.materializing_digest.is_none() || value.object_terminal_digest.is_none() {
                    return Err(ArtifactContractError::InvalidState);
                }
            }
            MaterializationTerminalStateV1::Failed => {
                if value.object_terminal_digest.is_some() {
                    return Err(ArtifactContractError::InvalidState);
                }
            }
            MaterializationTerminalStateV1::Uncertain => {
                if value.materializing_digest.is_none() || value.object_terminal_digest.is_some() {
                    return Err(ArtifactContractError::InvalidState);
                }
            }
        }
        require_digest(
            value.terminal_digest,
            OPERATION_TERMINAL_DIGEST_DOMAIN,
            &bytes[..272],
        )?;
        canonical_round_trip(bytes, &value.encode())?;
        Ok(value)
    }

    fn encode_prefix(&self) -> [u8; 272] {
        let mut bytes = [0_u8; 272];
        put_frame_prefix(&mut bytes, PXAW_MAGIC, b'M', self.state.byte());
        put_u16(&mut bytes, 8, PXAW_BYTES as u16);
        put_u32(&mut bytes, 12, PXAW_BYTES as u32);
        bytes[16..48].copy_from_slice(self.store_instance.as_bytes());
        put_u64(&mut bytes, 48, self.operation_sequence.get());
        bytes[56..72].copy_from_slice(self.operation_id.as_bytes());
        bytes[72..104].copy_from_slice(self.request_digest.as_bytes());
        bytes[104..136].copy_from_slice(self.admission_digest.as_bytes());
        if let Some(digest) = self.materializing_digest {
            bytes[136..168].copy_from_slice(digest.as_bytes());
        }
        if let Some(digest) = self.object_terminal_digest {
            bytes[168..200].copy_from_slice(digest.as_bytes());
        }
        bytes[200..272].copy_from_slice(&self.object_ref.encode());
        bytes
    }

    #[must_use]
    pub fn encode(&self) -> [u8; PXAW_BYTES] {
        let mut bytes = [0_u8; PXAW_BYTES];
        bytes[..272].copy_from_slice(&self.encode_prefix());
        bytes[272..304].copy_from_slice(self.terminal_digest.as_bytes());
        bytes
    }

    #[must_use]
    pub const fn store_instance(&self) -> ArtifactStoreInstanceV1 {
        self.store_instance
    }
    #[must_use]
    pub const fn operation_sequence(&self) -> NonZeroU64 {
        self.operation_sequence
    }
    #[must_use]
    pub const fn operation_id(&self) -> ArtifactOperationIdV1 {
        self.operation_id
    }
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }
    #[must_use]
    pub const fn admission_digest(&self) -> Digest32 {
        self.admission_digest
    }
    #[must_use]
    pub const fn materializing_digest(&self) -> Option<Digest32> {
        self.materializing_digest
    }
    #[must_use]
    pub const fn object_terminal_digest(&self) -> Option<Digest32> {
        self.object_terminal_digest
    }
    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }
    #[must_use]
    pub const fn state(&self) -> MaterializationTerminalStateV1 {
        self.state
    }
    #[must_use]
    pub const fn terminal_digest(&self) -> Digest32 {
        self.terminal_digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationReceiptV1 {
    store_instance: ArtifactStoreInstanceV1,
    operation_sequence: NonZeroU64,
    operation_id: ArtifactOperationIdV1,
    request_digest: Digest32,
    terminal_digest: Digest32,
    object_ref: ArtifactObjectRefV1,
    state: MaterializationTerminalStateV1,
    receipt_digest: Digest32,
}

impl MaterializationReceiptV1 {
    #[must_use]
    pub fn new(terminal: &MaterializationTerminalV1) -> Self {
        let mut value = Self {
            store_instance: terminal.store_instance(),
            operation_sequence: terminal.operation_sequence(),
            operation_id: terminal.operation_id(),
            request_digest: terminal.request_digest(),
            terminal_digest: terminal.terminal_digest(),
            object_ref: terminal.object_ref(),
            state: terminal.state(),
            receipt_digest: Digest32::from_bytes([0; 32]),
        };
        let bytes = value.encode_prefix();
        value.receipt_digest = raw_sha256(RECEIPT_DIGEST_DOMAIN, &[&bytes]);
        value
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        if bytes.len() != PXAX_BYTES {
            return Err(ArtifactContractError::InvalidLength);
        }
        let state = MaterializationTerminalStateV1::from_byte(bytes[7])?;
        check_frame_prefix(bytes, PXAX_BYTES, PXAX_MAGIC, b'M', state.byte())?;
        let value = Self {
            store_instance: ArtifactStoreInstanceV1::try_from_bytes(read_array(bytes, 16)?)?,
            operation_sequence: NonZeroU64::new(read_u64(bytes, 48)?)
                .ok_or(ArtifactContractError::ZeroSequence)?,
            operation_id: ArtifactOperationIdV1::try_from_bytes(read_array(bytes, 56)?)?,
            request_digest: nonzero_digest(read_array(bytes, 72)?)?,
            terminal_digest: nonzero_digest(read_array(bytes, 104)?)?,
            object_ref: ArtifactObjectRefV1::decode(&bytes[136..208])?,
            state,
            receipt_digest: Digest32::from_bytes(read_array(bytes, 208)?),
        };
        require_digest(value.receipt_digest, RECEIPT_DIGEST_DOMAIN, &bytes[..208])?;
        canonical_round_trip(bytes, &value.encode())?;
        Ok(value)
    }

    fn encode_prefix(&self) -> [u8; 208] {
        let mut bytes = [0_u8; 208];
        put_frame_prefix(&mut bytes, PXAX_MAGIC, b'M', self.state.byte());
        put_u16(&mut bytes, 8, PXAX_BYTES as u16);
        put_u32(&mut bytes, 12, PXAX_BYTES as u32);
        bytes[16..48].copy_from_slice(self.store_instance.as_bytes());
        put_u64(&mut bytes, 48, self.operation_sequence.get());
        bytes[56..72].copy_from_slice(self.operation_id.as_bytes());
        bytes[72..104].copy_from_slice(self.request_digest.as_bytes());
        bytes[104..136].copy_from_slice(self.terminal_digest.as_bytes());
        bytes[136..208].copy_from_slice(&self.object_ref.encode());
        bytes
    }

    #[must_use]
    pub fn encode(&self) -> [u8; PXAX_BYTES] {
        let mut bytes = [0_u8; PXAX_BYTES];
        bytes[..208].copy_from_slice(&self.encode_prefix());
        bytes[208..240].copy_from_slice(self.receipt_digest.as_bytes());
        bytes
    }

    #[must_use]
    pub const fn store_instance(&self) -> ArtifactStoreInstanceV1 {
        self.store_instance
    }
    #[must_use]
    pub const fn operation_sequence(&self) -> NonZeroU64 {
        self.operation_sequence
    }
    #[must_use]
    pub const fn operation_id(&self) -> ArtifactOperationIdV1 {
        self.operation_id
    }
    #[must_use]
    pub const fn request_digest(&self) -> Digest32 {
        self.request_digest
    }
    #[must_use]
    pub const fn terminal_digest(&self) -> Digest32 {
        self.terminal_digest
    }
    #[must_use]
    pub const fn object_ref(&self) -> ArtifactObjectRefV1 {
        self.object_ref
    }
    #[must_use]
    pub const fn state(&self) -> MaterializationTerminalStateV1 {
        self.state
    }
    #[must_use]
    pub const fn receipt_digest(&self) -> Digest32 {
        self.receipt_digest
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MaterializationReceiptRefV1 {
    store_instance: ArtifactStoreInstanceV1,
    operation_sequence: NonZeroU64,
    operation_id: ArtifactOperationIdV1,
    receipt_digest: Digest32,
}

impl MaterializationReceiptRefV1 {
    #[must_use]
    pub const fn from_receipt(receipt: &MaterializationReceiptV1) -> Self {
        Self {
            store_instance: receipt.store_instance(),
            operation_sequence: receipt.operation_sequence(),
            operation_id: receipt.operation_id(),
            receipt_digest: receipt.receipt_digest(),
        }
    }

    #[must_use]
    pub const fn store_instance(&self) -> ArtifactStoreInstanceV1 {
        self.store_instance
    }
    #[must_use]
    pub const fn operation_sequence(&self) -> NonZeroU64 {
        self.operation_sequence
    }
    #[must_use]
    pub const fn operation_id(&self) -> ArtifactOperationIdV1 {
        self.operation_id
    }
    #[must_use]
    pub const fn receipt_digest(&self) -> Digest32 {
        self.receipt_digest
    }
}

impl fmt::Display for MaterializationReceiptRefV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut output = String::with_capacity(7 + 64 + 1 + 20 + 1 + 32 + 1 + 64);
        output.push_str("pxamr1:");
        push_lower_hex(&mut output, self.store_instance.as_bytes());
        output.push(':');
        output.push_str(&self.operation_sequence.get().to_string());
        output.push(':');
        push_lower_hex(&mut output, self.operation_id.as_bytes());
        output.push(':');
        push_lower_hex(&mut output, self.receipt_digest.as_bytes());
        formatter.write_str(&output)
    }
}

impl FromStr for MaterializationReceiptRefV1 {
    type Err = ArtifactContractError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut fields = input.split(':');
        if fields.next() != Some("pxamr1") {
            return Err(ArtifactContractError::InvalidReference);
        }
        let store = fields
            .next()
            .ok_or(ArtifactContractError::InvalidReference)?;
        let sequence = fields
            .next()
            .ok_or(ArtifactContractError::InvalidReference)?;
        let operation = fields
            .next()
            .ok_or(ArtifactContractError::InvalidReference)?;
        let receipt = fields
            .next()
            .ok_or(ArtifactContractError::InvalidReference)?;
        if fields.next().is_some()
            || sequence.is_empty()
            || (sequence.len() > 1 && sequence.starts_with('0'))
            || !sequence.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(ArtifactContractError::InvalidReference);
        }
        let value = Self {
            store_instance: ArtifactStoreInstanceV1::try_from_bytes(decode_lower_hex(store)?)?,
            operation_sequence: NonZeroU64::new(
                sequence
                    .parse()
                    .map_err(|_| ArtifactContractError::InvalidReference)?,
            )
            .ok_or(ArtifactContractError::InvalidReference)?,
            operation_id: ArtifactOperationIdV1::try_from_bytes(decode_lower_hex(operation)?)?,
            receipt_digest: nonzero_digest(decode_lower_hex(receipt)?)?,
        };
        if value.to_string() != input {
            return Err(ArtifactContractError::InvalidReference);
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterializationOperationV1 {
    request: MaterializationRequestV1,
    admission: MaterializationAdmissionV1,
    materializing: Option<MaterializingRecordV1>,
    terminal: Option<MaterializationTerminalV1>,
    receipt: Option<MaterializationReceiptV1>,
}

impl MaterializationOperationV1 {
    pub fn admitted(
        request: MaterializationRequestV1,
        admission: MaterializationAdmissionV1,
    ) -> Result<Self, ArtifactContractError> {
        let value = Self {
            request,
            admission,
            materializing: None,
            terminal: None,
            receipt: None,
        };
        value.validate_presence()?;
        Ok(value)
    }

    pub fn decode_canonical(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        if bytes.len() < PXOP_HEADER_BYTES {
            return Err(ArtifactContractError::InvalidLength);
        }
        if bytes.get(0..4) != Some(PXOP_MAGIC.as_slice()) {
            return Err(ArtifactContractError::InvalidMagic);
        }
        if read_u16(bytes, 4)? != 1 {
            return Err(ArtifactContractError::UnsupportedVersion);
        }
        if read_u16(bytes, 6)? != PXOP_HEADER_BYTES as u16 {
            return Err(ArtifactContractError::InvalidHeader);
        }
        let entry_len = usize::try_from(read_u32(bytes, 8)?)
            .map_err(|_| ArtifactContractError::InvalidLength)?;
        let flags = read_u32(bytes, 12)?;
        if entry_len != bytes.len() || entry_len != operation_entry_len(flags)? {
            return Err(ArtifactContractError::InvalidLength);
        }
        let header_operation_id = ArtifactOperationIdV1::try_from_bytes(read_array(bytes, 16)?)?;
        let mut cursor = PXOP_HEADER_BYTES;
        let request = MaterializationRequestV1::decode(&bytes[cursor..cursor + PXAQ_BYTES])?;
        cursor += PXAQ_BYTES;
        let admission = MaterializationAdmissionV1::decode(&bytes[cursor..cursor + PXAA_BYTES])?;
        cursor += PXAA_BYTES;
        let materializing = if flags & 1 != 0 {
            let value = MaterializingRecordV1::decode(&bytes[cursor..cursor + PXMU_BYTES])?;
            cursor += PXMU_BYTES;
            Some(value)
        } else {
            None
        };
        let terminal = if flags & 2 != 0 {
            let value = MaterializationTerminalV1::decode(&bytes[cursor..cursor + PXAW_BYTES])?;
            cursor += PXAW_BYTES;
            Some(value)
        } else {
            None
        };
        let receipt = if flags & 4 != 0 {
            let value = MaterializationReceiptV1::decode(&bytes[cursor..cursor + PXAX_BYTES])?;
            cursor += PXAX_BYTES;
            Some(value)
        } else {
            None
        };
        if cursor != bytes.len() {
            return Err(ArtifactContractError::InvalidLength);
        }
        let value = Self {
            request,
            admission,
            materializing,
            terminal,
            receipt,
        };
        value.validate_presence()?;
        if header_operation_id != value.operation_id() {
            return Err(ArtifactContractError::CrossFrameMismatch);
        }
        if value.encode_canonical()?.as_ref() != bytes {
            return Err(ArtifactContractError::NonCanonicalEncoding);
        }
        Ok(value)
    }

    fn validate_presence(&self) -> Result<(), ArtifactContractError> {
        let flags = self.presence_flags();
        operation_entry_len(flags)?;
        if self.request.operation_id() != self.admission.operation_id()
            || self.request.request_digest() != self.admission.request_digest()
            || self.request.object_ref() != self.admission.object_ref()
        {
            return Err(ArtifactContractError::CrossFrameMismatch);
        }
        if let Some(materializing) = &self.materializing
            && (materializing.store_instance() != self.admission.store_instance()
                || materializing.operation_sequence() != self.admission.operation_sequence()
                || materializing.operation_id() != self.operation_id()
                || materializing.request_digest() != self.request.request_digest()
                || materializing.admission_digest() != self.admission.admission_digest()
                || materializing.object_ref() != self.request.object_ref())
        {
            return Err(ArtifactContractError::CrossFrameMismatch);
        }
        if let Some(terminal) = &self.terminal
            && (terminal.store_instance() != self.admission.store_instance()
                || terminal.operation_sequence() != self.admission.operation_sequence()
                || terminal.operation_id() != self.operation_id()
                || terminal.request_digest() != self.request.request_digest()
                || terminal.admission_digest() != self.admission.admission_digest()
                || terminal.object_ref() != self.request.object_ref()
                || terminal.materializing_digest()
                    != self
                        .materializing
                        .as_ref()
                        .map(MaterializingRecordV1::materializing_digest))
        {
            return Err(ArtifactContractError::CrossFrameMismatch);
        }
        if let Some(receipt) = &self.receipt {
            let terminal = self
                .terminal
                .as_ref()
                .ok_or(ArtifactContractError::InvalidState)?;
            if receipt.store_instance() != self.admission.store_instance()
                || receipt.operation_sequence() != self.admission.operation_sequence()
                || receipt.operation_id() != self.operation_id()
                || receipt.request_digest() != self.request.request_digest()
                || receipt.terminal_digest() != terminal.terminal_digest()
                || receipt.object_ref() != self.request.object_ref()
                || receipt.state() != terminal.state()
            {
                return Err(ArtifactContractError::CrossFrameMismatch);
            }
        }
        Ok(())
    }

    fn presence_flags(&self) -> u32 {
        u32::from(self.materializing.is_some())
            | (u32::from(self.terminal.is_some()) << 1)
            | (u32::from(self.receipt.is_some()) << 2)
    }

    pub fn encode_canonical(&self) -> Result<Box<[u8]>, ArtifactContractError> {
        self.validate_presence()?;
        let flags = self.presence_flags();
        let entry_len = operation_entry_len(flags)?;
        let mut bytes = vec![0_u8; entry_len];
        bytes[0..4].copy_from_slice(PXOP_MAGIC);
        put_u16(&mut bytes, 4, 1);
        put_u16(&mut bytes, 6, PXOP_HEADER_BYTES as u16);
        put_u32(&mut bytes, 8, entry_len as u32);
        put_u32(&mut bytes, 12, flags);
        bytes[16..32].copy_from_slice(self.operation_id().as_bytes());
        let mut cursor = PXOP_HEADER_BYTES;
        bytes[cursor..cursor + PXAQ_BYTES].copy_from_slice(&self.request.encode());
        cursor += PXAQ_BYTES;
        bytes[cursor..cursor + PXAA_BYTES].copy_from_slice(&self.admission.encode());
        cursor += PXAA_BYTES;
        if let Some(materializing) = &self.materializing {
            bytes[cursor..cursor + PXMU_BYTES].copy_from_slice(&materializing.encode());
            cursor += PXMU_BYTES;
        }
        if let Some(terminal) = &self.terminal {
            bytes[cursor..cursor + PXAW_BYTES].copy_from_slice(&terminal.encode());
            cursor += PXAW_BYTES;
        }
        if let Some(receipt) = &self.receipt {
            bytes[cursor..cursor + PXAX_BYTES].copy_from_slice(&receipt.encode());
        }
        Ok(bytes.into_boxed_slice())
    }

    #[must_use]
    pub const fn operation_id(&self) -> ArtifactOperationIdV1 {
        self.request.operation_id()
    }
    #[must_use]
    pub const fn request(&self) -> &MaterializationRequestV1 {
        &self.request
    }
    #[must_use]
    pub const fn admission(&self) -> &MaterializationAdmissionV1 {
        &self.admission
    }
    #[must_use]
    pub const fn materializing(&self) -> Option<&MaterializingRecordV1> {
        self.materializing.as_ref()
    }
    #[must_use]
    pub const fn terminal(&self) -> Option<&MaterializationTerminalV1> {
        self.terminal.as_ref()
    }
    #[must_use]
    pub const fn receipt(&self) -> Option<&MaterializationReceiptV1> {
        self.receipt.as_ref()
    }
}

fn operation_entry_len(flags: u32) -> Result<usize, ArtifactContractError> {
    match flags {
        0 => Ok(416),
        1 => Ok(656),
        2 => Ok(720),
        3 => Ok(960),
        6 => Ok(960),
        7 => Ok(1200),
        _ => Err(ArtifactContractError::InvalidState),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactQuarantineFactsV1 {
    Absent,
    Present {
        operation_id: ArtifactOperationIdV1,
        regular_file_bytes: u64,
    },
}

impl ArtifactQuarantineFactsV1 {
    #[must_use]
    pub const fn regular_file_bytes(self) -> u64 {
        match self {
            Self::Absent => 0,
            Self::Present {
                regular_file_bytes, ..
            } => regular_file_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArtifactCapacityInputV1 {
    accounted_before: u64,
    snapshot_frame_bytes: u64,
    new_pair_bytes_not_already_counted: u64,
    known_owner_temp_bytes_not_already_counted: u64,
    object_count: usize,
    operation_count: usize,
    quarantine_bytes: u64,
}

impl ArtifactCapacityInputV1 {
    #[must_use]
    pub const fn new(
        accounted_before: u64,
        snapshot_frame_bytes: u64,
        new_pair_bytes_not_already_counted: u64,
        known_owner_temp_bytes_not_already_counted: u64,
        object_count: usize,
        operation_count: usize,
        quarantine_bytes: u64,
    ) -> Self {
        Self {
            accounted_before,
            snapshot_frame_bytes,
            new_pair_bytes_not_already_counted,
            known_owner_temp_bytes_not_already_counted,
            object_count,
            operation_count,
            quarantine_bytes,
        }
    }

    pub fn checked_total(self) -> Result<u64, ArtifactContractError> {
        if self.snapshot_frame_bytes > MAX_ARTIFACT_SNAPSHOT_BYTES as u64
            || self.object_count > MAX_ARTIFACT_OBJECTS
            || self.operation_count > MAX_ARTIFACT_OPERATIONS
            || self.quarantine_bytes > MAX_ARTIFACT_QUARANTINE_BYTES
        {
            return Err(ArtifactContractError::CapacityExceeded);
        }
        let total = self
            .accounted_before
            .checked_add(self.snapshot_frame_bytes)
            .and_then(|value| value.checked_add(self.new_pair_bytes_not_already_counted))
            .and_then(|value| value.checked_add(self.known_owner_temp_bytes_not_already_counted))
            .ok_or(ArtifactContractError::ArithmeticOverflow)?;
        if total > ARTIFACT_DEFENSE_CEILING_BYTES {
            return Err(ArtifactContractError::CapacityExceeded);
        }
        Ok(total)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedMaterializationReadBundleV1 {
    request: MaterializationRequestV1,
    admission: MaterializationAdmissionV1,
    materializing: Option<MaterializingRecordV1>,
    object: Option<ArtifactObjectRecordV1>,
    terminal: MaterializationTerminalV1,
    receipt: MaterializationReceiptV1,
    pair: Option<VerifiedArtifactPairV1>,
}

impl VerifiedMaterializationReadBundleV1 {
    fn verify(
        request: MaterializationRequestV1,
        admission: MaterializationAdmissionV1,
        materializing: Option<MaterializingRecordV1>,
        object: Option<ArtifactObjectRecordV1>,
        terminal: MaterializationTerminalV1,
        receipt: MaterializationReceiptV1,
        pair: Option<VerifiedArtifactPairV1>,
    ) -> Result<Self, ArtifactContractError> {
        let operation = MaterializationOperationV1 {
            request: request.clone(),
            admission: admission.clone(),
            materializing: materializing.clone(),
            terminal: Some(terminal.clone()),
            receipt: Some(receipt.clone()),
        };
        operation.validate_presence()?;
        match terminal.state() {
            MaterializationTerminalStateV1::Materialized
            | MaterializationTerminalStateV1::AlreadyMaterialized => {
                let materializing = materializing
                    .as_ref()
                    .ok_or(ArtifactContractError::CrossFrameMismatch)?;
                let object = object
                    .as_ref()
                    .ok_or(ArtifactContractError::CrossFrameMismatch)?;
                let pair = pair
                    .as_ref()
                    .ok_or(ArtifactContractError::CrossFrameMismatch)?;
                if terminal.materializing_digest() != Some(materializing.materializing_digest())
                    || terminal.object_terminal_digest() != Some(object.object_terminal_digest())
                    || object.store_instance() != admission.store_instance()
                    || object.object_ref() != request.object_ref()
                    || object.object_ref() != pair.object_ref()
                    || object.payload_len() != pair.manifest().payload_len()
                {
                    return Err(ArtifactContractError::CrossFrameMismatch);
                }
            }
            MaterializationTerminalStateV1::Failed | MaterializationTerminalStateV1::Uncertain => {
                if object.is_some() || pair.is_some() {
                    return Err(ArtifactContractError::CrossFrameMismatch);
                }
                if terminal.materializing_digest()
                    != materializing
                        .as_ref()
                        .map(MaterializingRecordV1::materializing_digest)
                {
                    return Err(ArtifactContractError::CrossFrameMismatch);
                }
            }
        }
        Ok(Self {
            request,
            admission,
            materializing,
            object,
            terminal,
            receipt,
            pair,
        })
    }

    #[must_use]
    pub const fn request(&self) -> &MaterializationRequestV1 {
        &self.request
    }
    #[must_use]
    pub const fn admission(&self) -> &MaterializationAdmissionV1 {
        &self.admission
    }
    #[must_use]
    pub const fn materializing(&self) -> Option<&MaterializingRecordV1> {
        self.materializing.as_ref()
    }
    #[must_use]
    pub const fn object(&self) -> Option<&ArtifactObjectRecordV1> {
        self.object.as_ref()
    }
    #[must_use]
    pub const fn terminal(&self) -> &MaterializationTerminalV1 {
        &self.terminal
    }
    #[must_use]
    pub const fn receipt(&self) -> &MaterializationReceiptV1 {
        &self.receipt
    }
    #[must_use]
    pub const fn pair(&self) -> Option<&VerifiedArtifactPairV1> {
        self.pair.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactStoreSnapshotV1 {
    store_instance: ArtifactStoreInstanceV1,
    config_commitment: ArtifactConfigCommitmentV1,
    snapshot_sequence: NonZeroU64,
    operation_high_water: u64,
    object_high_water: u64,
    objects: Vec<ArtifactObjectRecordV1>,
    operations: Vec<MaterializationOperationV1>,
    quarantine: ArtifactQuarantineFactsV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactFilesystemClaimV1 {
    Stable,
    Materializing {
        operation_id: ArtifactOperationIdV1,
        object_ref: ArtifactObjectRefV1,
    },
    Quarantined {
        operation_id: ArtifactOperationIdV1,
        object_ref: ArtifactObjectRefV1,
        regular_file_bytes: u64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactStoreSnapshotCandidateV1 {
    snapshot: ArtifactStoreSnapshotV1,
    filesystem_claim: ArtifactFilesystemClaimV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactRecoveryStartV1 {
    NoMatchingObject,
    UnreferencedCurrent(ArtifactObjectRecordV1),
    EarlierReferenced(ArtifactObjectRecordV1),
}

impl ArtifactStoreSnapshotCandidateV1 {
    pub fn decode_canonical(bytes: &[u8]) -> Result<Self, ArtifactContractError> {
        let quarantine = infer_quarantine_facts(bytes)?;
        let snapshot = ArtifactStoreSnapshotV1::decode_with_quarantine(bytes, quarantine)?;
        let filesystem_claim = match quarantine {
            ArtifactQuarantineFactsV1::Present {
                operation_id,
                regular_file_bytes,
            } => {
                let operation = snapshot
                    .operation(operation_id)
                    .ok_or(ArtifactContractError::InvalidQuarantineFacts)?;
                ArtifactFilesystemClaimV1::Quarantined {
                    operation_id,
                    object_ref: operation.request().object_ref(),
                    regular_file_bytes,
                }
            }
            ArtifactQuarantineFactsV1::Absent => snapshot
                .operations
                .last()
                .filter(|operation| {
                    operation.materializing().is_some() && operation.terminal().is_none()
                })
                .map_or(ArtifactFilesystemClaimV1::Stable, |operation| {
                    ArtifactFilesystemClaimV1::Materializing {
                        operation_id: operation.operation_id(),
                        object_ref: operation.request().object_ref(),
                    }
                }),
        };
        Ok(Self {
            snapshot,
            filesystem_claim,
        })
    }

    #[must_use]
    pub const fn filesystem_claim(&self) -> &ArtifactFilesystemClaimV1 {
        &self.filesystem_claim
    }

    #[must_use]
    pub const fn store_instance(&self) -> ArtifactStoreInstanceV1 {
        self.snapshot.store_instance()
    }

    #[must_use]
    pub const fn config_commitment(&self) -> ArtifactConfigCommitmentV1 {
        self.snapshot.config_commitment()
    }

    #[must_use]
    pub const fn snapshot_sequence(&self) -> NonZeroU64 {
        self.snapshot.snapshot_sequence()
    }

    #[must_use]
    pub fn objects(&self) -> &[ArtifactObjectRecordV1] {
        self.snapshot.objects()
    }

    #[must_use]
    pub fn operations(&self) -> &[MaterializationOperationV1] {
        self.snapshot.operations()
    }

    #[must_use]
    pub fn operation(
        &self,
        operation_id: ArtifactOperationIdV1,
    ) -> Option<&MaterializationOperationV1> {
        self.snapshot.operation(operation_id)
    }

    pub fn validate_filesystem(
        self,
        facts: ArtifactFilesystemClaimV1,
    ) -> Result<ArtifactStoreSnapshotV1, ArtifactContractError> {
        if self.filesystem_claim != facts {
            return Err(ArtifactContractError::InvalidQuarantineFacts);
        }
        Ok(self.snapshot)
    }
}

fn infer_quarantine_facts(
    bytes: &[u8],
) -> Result<ArtifactQuarantineFactsV1, ArtifactContractError> {
    if bytes.len() < PXAZ_HEADER_BYTES {
        return Err(ArtifactContractError::InvalidLength);
    }
    let flags = read_u32(bytes, 24)?;
    if flags == 0 {
        return Ok(ArtifactQuarantineFactsV1::Absent);
    }
    if flags != OBJECT_PUBLICATION_BLOCKED {
        return Err(ArtifactContractError::InvalidHeader);
    }
    let body = bytes
        .get(PXAZ_HEADER_BYTES..)
        .ok_or(ArtifactContractError::InvalidLength)?;
    if body.len() < PXAY_HEADER_BYTES {
        return Err(ArtifactContractError::InvalidLength);
    }
    let object_count =
        usize::try_from(read_u32(body, 16)?).map_err(|_| ArtifactContractError::InvalidLength)?;
    let operation_count =
        usize::try_from(read_u32(body, 20)?).map_err(|_| ArtifactContractError::InvalidLength)?;
    if operation_count == 0
        || object_count > MAX_ARTIFACT_OBJECTS
        || operation_count > MAX_ARTIFACT_OPERATIONS
    {
        return Err(ArtifactContractError::InvalidSnapshot);
    }
    let mut cursor = PXAY_HEADER_BYTES
        .checked_add(
            object_count
                .checked_mul(PXAV_BYTES)
                .ok_or(ArtifactContractError::ArithmeticOverflow)?,
        )
        .ok_or(ArtifactContractError::ArithmeticOverflow)?;
    let mut last_operation_id = None;
    for _ in 0..operation_count {
        let entry_len = usize::try_from(read_u32(body, cursor + 8)?)
            .map_err(|_| ArtifactContractError::InvalidLength)?;
        last_operation_id = Some(ArtifactOperationIdV1::try_from_bytes(read_array(
            body,
            cursor + 16,
        )?)?);
        cursor = cursor
            .checked_add(entry_len)
            .ok_or(ArtifactContractError::InvalidLength)?;
        if cursor > body.len() {
            return Err(ArtifactContractError::InvalidLength);
        }
    }
    Ok(ArtifactQuarantineFactsV1::Present {
        operation_id: last_operation_id.ok_or(ArtifactContractError::InvalidSnapshot)?,
        regular_file_bytes: read_u64(bytes, 144)?,
    })
}

impl ArtifactStoreSnapshotV1 {
    pub fn initial(
        store_instance: ArtifactStoreInstanceV1,
        config_commitment: ArtifactConfigCommitmentV1,
        request: MaterializationRequestV1,
        admission: MaterializationAdmissionV1,
    ) -> Result<Self, ArtifactContractError> {
        if request.config_commitment() != config_commitment
            || admission.store_instance() != store_instance
            || admission.operation_sequence().get() != 1
        {
            return Err(ArtifactContractError::CrossFrameMismatch);
        }
        let operation = MaterializationOperationV1::admitted(request, admission)?;
        let value = Self {
            store_instance,
            config_commitment,
            snapshot_sequence: NonZeroU64::new(1).expect("one is nonzero"),
            operation_high_water: 1,
            object_high_water: 0,
            objects: Vec::new(),
            operations: vec![operation],
            quarantine: ArtifactQuarantineFactsV1::Absent,
        };
        value.validate()?;
        value.ensure_capacity()?;
        Ok(value)
    }

    fn decode_with_quarantine(
        bytes: &[u8],
        quarantine: ArtifactQuarantineFactsV1,
    ) -> Result<Self, ArtifactContractError> {
        if bytes.len() < PXAZ_HEADER_BYTES || bytes.len() > MAX_ARTIFACT_SNAPSHOT_BYTES {
            return Err(ArtifactContractError::InvalidLength);
        }
        if bytes.get(0..4) != Some(PXAZ_MAGIC.as_slice()) {
            return Err(ArtifactContractError::InvalidMagic);
        }
        if read_u16(bytes, 4)? != 1 {
            return Err(ArtifactContractError::UnsupportedVersion);
        }
        if read_u16(bytes, 6)? != PXAZ_HEADER_BYTES as u16
            || read_u64(bytes, 8)? != bytes.len() as u64
            || read_u16(bytes, 16)? != 1
            || read_u16(bytes, 18)? != 1
            || read_u16(bytes, 20)? != 1
            || read_u16(bytes, 22)? != 1
        {
            return Err(ArtifactContractError::InvalidHeader);
        }
        let state_flags = read_u32(bytes, 24)?;
        if state_flags & !OBJECT_PUBLICATION_BLOCKED != 0 {
            return Err(ArtifactContractError::InvalidHeader);
        }
        require_zero(&bytes[28..32])?;
        require_zero(&bytes[152..160])?;
        let operation_count = usize::try_from(read_u32(bytes, 120)?)
            .map_err(|_| ArtifactContractError::InvalidLength)?;
        let object_count = usize::try_from(read_u32(bytes, 124)?)
            .map_err(|_| ArtifactContractError::InvalidLength)?;
        if operation_count > MAX_ARTIFACT_OPERATIONS || object_count > MAX_ARTIFACT_OBJECTS {
            return Err(ArtifactContractError::CapacityExceeded);
        }
        let body_len = usize::try_from(read_u64(bytes, 128)?)
            .map_err(|_| ArtifactContractError::InvalidLength)?;
        if body_len > MAX_PXAY_BODY_BYTES
            || PXAZ_HEADER_BYTES.checked_add(body_len) != Some(bytes.len())
        {
            return Err(ArtifactContractError::InvalidLength);
        }
        let quarantine_bytes = read_u64(bytes, 144)?;
        let expected_flag = match quarantine {
            ArtifactQuarantineFactsV1::Absent => {
                if quarantine_bytes != 0 {
                    return Err(ArtifactContractError::InvalidQuarantineFacts);
                }
                0
            }
            ArtifactQuarantineFactsV1::Present {
                regular_file_bytes, ..
            } => {
                if regular_file_bytes != quarantine_bytes
                    || regular_file_bytes > MAX_ARTIFACT_QUARANTINE_BYTES
                {
                    return Err(ArtifactContractError::InvalidQuarantineFacts);
                }
                OBJECT_PUBLICATION_BLOCKED
            }
        };
        if state_flags != expected_flag {
            return Err(ArtifactContractError::InvalidQuarantineFacts);
        }
        let body = &bytes[PXAZ_HEADER_BYTES..];
        let length_prefix = (160_u64).to_be_bytes();
        let body_length_prefix = (body_len as u64).to_be_bytes();
        let checksum = raw_sha256(
            SNAPSHOT_DIGEST_DOMAIN,
            &[&length_prefix, &bytes[..160], &body_length_prefix, body],
        );
        if checksum.as_bytes().as_slice() != &bytes[160..192] {
            return Err(ArtifactContractError::DigestMismatch);
        }
        if body.len() < PXAY_HEADER_BYTES || body.get(0..4) != Some(PXAY_MAGIC.as_slice()) {
            return Err(ArtifactContractError::InvalidMagic);
        }
        if read_u16(body, 4)? != 1 {
            return Err(ArtifactContractError::UnsupportedVersion);
        }
        if read_u16(body, 6)? != PXAY_HEADER_BYTES as u16
            || read_u64(body, 8)? != body_len as u64
            || read_u32(body, 16)? != object_count as u32
            || read_u32(body, 20)? != operation_count as u32
        {
            return Err(ArtifactContractError::InvalidHeader);
        }
        require_zero(&body[40..64])?;
        let object_table_len = object_count
            .checked_mul(PXAV_BYTES)
            .ok_or(ArtifactContractError::ArithmeticOverflow)?;
        if read_u64(body, 24)? != object_table_len as u64 {
            return Err(ArtifactContractError::InvalidLength);
        }
        let operation_table_len = usize::try_from(read_u64(body, 32)?)
            .map_err(|_| ArtifactContractError::InvalidLength)?;
        if PXAY_HEADER_BYTES
            .checked_add(object_table_len)
            .and_then(|value| value.checked_add(operation_table_len))
            != Some(body.len())
        {
            return Err(ArtifactContractError::InvalidLength);
        }
        let mut cursor = PXAY_HEADER_BYTES;
        let mut objects = Vec::with_capacity(object_count);
        for _ in 0..object_count {
            objects.push(ArtifactObjectRecordV1::decode(
                &body[cursor..cursor + PXAV_BYTES],
            )?);
            cursor += PXAV_BYTES;
        }
        let mut operations = Vec::with_capacity(operation_count);
        for _ in 0..operation_count {
            let entry_len = usize::try_from(read_u32(body, cursor + 8)?)
                .map_err(|_| ArtifactContractError::InvalidLength)?;
            let end = cursor
                .checked_add(entry_len)
                .ok_or(ArtifactContractError::InvalidLength)?;
            operations.push(MaterializationOperationV1::decode_canonical(
                body.get(cursor..end)
                    .ok_or(ArtifactContractError::InvalidLength)?,
            )?);
            cursor = end;
        }
        if cursor != body.len() {
            return Err(ArtifactContractError::InvalidLength);
        }
        let value = Self {
            store_instance: ArtifactStoreInstanceV1::try_from_bytes(read_array(bytes, 32)?)?,
            config_commitment: ArtifactConfigCommitmentV1::try_from_bytes(read_array(bytes, 64)?)?,
            snapshot_sequence: NonZeroU64::new(read_u64(bytes, 96)?)
                .ok_or(ArtifactContractError::ZeroSequence)?,
            operation_high_water: read_u64(bytes, 104)?,
            object_high_water: read_u64(bytes, 112)?,
            objects,
            operations,
            quarantine,
        };
        value.validate()?;
        if value.accounted_rest_bytes()? != read_u64(bytes, 136)? {
            return Err(ArtifactContractError::InvalidSnapshot);
        }
        if value.encode_canonical()?.as_ref() != bytes {
            return Err(ArtifactContractError::NonCanonicalEncoding);
        }
        Ok(value)
    }

    pub fn encode_canonical(&self) -> Result<Box<[u8]>, ArtifactContractError> {
        self.validate()?;
        let mut body = Vec::with_capacity(self.body_len()?);
        body.resize(PXAY_HEADER_BYTES, 0);
        body[0..4].copy_from_slice(PXAY_MAGIC);
        put_u16(&mut body, 4, 1);
        put_u16(&mut body, 6, PXAY_HEADER_BYTES as u16);
        put_u32(&mut body, 16, self.objects.len() as u32);
        put_u32(&mut body, 20, self.operations.len() as u32);
        put_u64(&mut body, 24, (self.objects.len() * PXAV_BYTES) as u64);
        for object in &self.objects {
            body.extend_from_slice(&object.encode());
        }
        let operation_start = body.len();
        for operation in &self.operations {
            body.extend_from_slice(&operation.encode_canonical()?);
        }
        let operation_table_len = body.len() - operation_start;
        let body_len = body.len();
        put_u64(&mut body, 32, operation_table_len as u64);
        put_u64(&mut body, 8, body_len as u64);
        if body.len() > MAX_PXAY_BODY_BYTES {
            return Err(ArtifactContractError::CapacityExceeded);
        }
        let frame_len = PXAZ_HEADER_BYTES
            .checked_add(body.len())
            .ok_or(ArtifactContractError::ArithmeticOverflow)?;
        let mut bytes = vec![0_u8; frame_len];
        bytes[0..4].copy_from_slice(PXAZ_MAGIC);
        put_u16(&mut bytes, 4, 1);
        put_u16(&mut bytes, 6, PXAZ_HEADER_BYTES as u16);
        put_u64(&mut bytes, 8, frame_len as u64);
        put_u16(&mut bytes, 16, 1);
        put_u16(&mut bytes, 18, 1);
        put_u16(&mut bytes, 20, 1);
        put_u16(&mut bytes, 22, 1);
        let quarantine_bytes = match self.quarantine {
            ArtifactQuarantineFactsV1::Absent => 0,
            ArtifactQuarantineFactsV1::Present {
                regular_file_bytes, ..
            } => {
                put_u32(&mut bytes, 24, OBJECT_PUBLICATION_BLOCKED);
                regular_file_bytes
            }
        };
        bytes[32..64].copy_from_slice(self.store_instance.as_bytes());
        bytes[64..96].copy_from_slice(self.config_commitment.as_bytes());
        put_u64(&mut bytes, 96, self.snapshot_sequence.get());
        put_u64(&mut bytes, 104, self.operation_high_water);
        put_u64(&mut bytes, 112, self.object_high_water);
        put_u32(&mut bytes, 120, self.operations.len() as u32);
        put_u32(&mut bytes, 124, self.objects.len() as u32);
        put_u64(&mut bytes, 128, body.len() as u64);
        put_u64(&mut bytes, 136, self.accounted_rest_bytes()?);
        put_u64(&mut bytes, 144, quarantine_bytes);
        bytes[PXAZ_HEADER_BYTES..].copy_from_slice(&body);
        let length_prefix = (160_u64).to_be_bytes();
        let body_length_prefix = (body.len() as u64).to_be_bytes();
        let checksum = raw_sha256(
            SNAPSHOT_DIGEST_DOMAIN,
            &[&length_prefix, &bytes[..160], &body_length_prefix, &body],
        );
        bytes[160..192].copy_from_slice(checksum.as_bytes());
        Ok(bytes.into_boxed_slice())
    }

    fn body_len(&self) -> Result<usize, ArtifactContractError> {
        let object_bytes = self
            .objects
            .len()
            .checked_mul(PXAV_BYTES)
            .ok_or(ArtifactContractError::ArithmeticOverflow)?;
        let operation_bytes = self.operations.iter().try_fold(
            0_usize,
            |total, operation| -> Result<usize, ArtifactContractError> {
                total
                    .checked_add(operation_entry_len(operation.presence_flags())?)
                    .ok_or(ArtifactContractError::ArithmeticOverflow)
            },
        )?;
        PXAY_HEADER_BYTES
            .checked_add(object_bytes)
            .and_then(|value| value.checked_add(operation_bytes))
            .ok_or(ArtifactContractError::ArithmeticOverflow)
    }

    fn frame_len(&self) -> Result<u64, ArtifactContractError> {
        let body_len = self.body_len()?;
        let frame_len = PXAZ_HEADER_BYTES
            .checked_add(body_len)
            .ok_or(ArtifactContractError::ArithmeticOverflow)?;
        u64::try_from(frame_len).map_err(|_| ArtifactContractError::ArithmeticOverflow)
    }

    pub fn accounted_rest_bytes(&self) -> Result<u64, ArtifactContractError> {
        let indexed = self.objects.iter().try_fold(0_u64, |total, object| {
            total
                .checked_add(PXAM_BYTES as u64)
                .and_then(|value| value.checked_add(object.payload_len()))
                .ok_or(ArtifactContractError::ArithmeticOverflow)
        })?;
        self.frame_len()?
            .checked_add(indexed)
            .and_then(|value| value.checked_add(self.quarantine.regular_file_bytes()))
            .ok_or(ArtifactContractError::ArithmeticOverflow)
    }

    fn ensure_capacity(&self) -> Result<(), ArtifactContractError> {
        let frame_len = self.frame_len()?;
        if frame_len > MAX_ARTIFACT_SNAPSHOT_BYTES as u64
            || self.accounted_rest_bytes()? > ARTIFACT_DEFENSE_CEILING_BYTES
        {
            return Err(ArtifactContractError::CapacityExceeded);
        }
        ArtifactCapacityInputV1::new(
            0,
            frame_len,
            0,
            0,
            self.objects.len(),
            self.operations.len(),
            self.quarantine.regular_file_bytes(),
        )
        .checked_total()
        .map(|_| ())
    }

    fn validate(&self) -> Result<(), ArtifactContractError> {
        let materializing_count = self.operations.iter().try_fold(0_u64, |count, operation| {
            if operation.materializing().is_some() {
                count.checked_add(1)
            } else {
                Some(count)
            }
            .ok_or(ArtifactContractError::ArithmeticOverflow)
        })?;
        let terminal_count = self.operations.iter().try_fold(0_u64, |count, operation| {
            if operation.terminal().is_some() {
                count.checked_add(1)
            } else {
                Some(count)
            }
            .ok_or(ArtifactContractError::ArithmeticOverflow)
        })?;
        let receipt_count = self.operations.iter().try_fold(0_u64, |count, operation| {
            if operation.receipt().is_some() {
                count.checked_add(1)
            } else {
                Some(count)
            }
            .ok_or(ArtifactContractError::ArithmeticOverflow)
        })?;
        let operation_count = u64::try_from(self.operations.len())
            .map_err(|_| ArtifactContractError::ArithmeticOverflow)?;
        let object_count = u64::try_from(self.objects.len())
            .map_err(|_| ArtifactContractError::ArithmeticOverflow)?;
        let expected_snapshot_sequence = operation_count
            .checked_add(materializing_count)
            .and_then(|value| value.checked_add(object_count))
            .and_then(|value| value.checked_add(terminal_count))
            .and_then(|value| value.checked_add(receipt_count))
            .ok_or(ArtifactContractError::ArithmeticOverflow)?;
        if self.operations.is_empty()
            || self.operations.len() > MAX_ARTIFACT_OPERATIONS
            || self.objects.len() > MAX_ARTIFACT_OBJECTS
            || self.operation_high_water != self.operations.len() as u64
            || self.object_high_water != self.objects.len() as u64
            || self.snapshot_sequence.get() != expected_snapshot_sequence
        {
            return Err(ArtifactContractError::InvalidSnapshot);
        }
        for (index, object) in self.objects.iter().enumerate() {
            if object.store_instance() != self.store_instance
                || object.object_sequence().get() != (index + 1) as u64
                || self.objects[..index]
                    .iter()
                    .any(|prior| prior.object_ref() == object.object_ref())
            {
                return Err(ArtifactContractError::InvalidSnapshot);
            }
        }
        let mut missing_receipt = None;
        for (index, operation) in self.operations.iter().enumerate() {
            operation.validate_presence()?;
            if operation.admission().store_instance() != self.store_instance
                || operation.request().config_commitment() != self.config_commitment
                || operation.admission().operation_sequence().get() != (index + 1) as u64
                || self.operations[..index]
                    .iter()
                    .any(|prior| prior.operation_id() == operation.operation_id())
            {
                return Err(ArtifactContractError::InvalidSnapshot);
            }
            if operation.receipt().is_none()
                && (missing_receipt.replace(index).is_some() || index + 1 != self.operations.len())
            {
                return Err(ArtifactContractError::InvalidSnapshot);
            }
            if let Some(terminal) = operation.terminal() {
                match terminal.state() {
                    MaterializationTerminalStateV1::Materialized
                    | MaterializationTerminalStateV1::AlreadyMaterialized => {
                        let digest = terminal
                            .object_terminal_digest()
                            .ok_or(ArtifactContractError::InvalidSnapshot)?;
                        let matches: Vec<_> = self
                            .objects
                            .iter()
                            .filter(|object| object.object_terminal_digest() == digest)
                            .collect();
                        if matches.len() != 1
                            || matches[0].object_ref() != operation.request().object_ref()
                        {
                            return Err(ArtifactContractError::InvalidSnapshot);
                        }
                        let referenced_earlier = self.operations[..index].iter().any(|prior| {
                            prior
                                .terminal()
                                .and_then(MaterializationTerminalV1::object_terminal_digest)
                                == Some(digest)
                        });
                        if (terminal.state() == MaterializationTerminalStateV1::Materialized
                            && referenced_earlier)
                            || (terminal.state()
                                == MaterializationTerminalStateV1::AlreadyMaterialized
                                && !referenced_earlier)
                        {
                            return Err(ArtifactContractError::InvalidSnapshot);
                        }
                    }
                    MaterializationTerminalStateV1::Failed => {
                        if terminal.object_terminal_digest().is_some()
                            || self.objects.iter().any(|object| {
                                object.object_ref() == operation.request().object_ref()
                            })
                        {
                            return Err(ArtifactContractError::InvalidSnapshot);
                        }
                    }
                    MaterializationTerminalStateV1::Uncertain => {
                        if index + 1 != self.operations.len()
                            || terminal.materializing_digest().is_none()
                            || terminal.object_terminal_digest().is_some()
                        {
                            return Err(ArtifactContractError::InvalidSnapshot);
                        }
                    }
                }
            }
        }
        let unreferenced: Vec<_> = self
            .objects
            .iter()
            .filter(|object| {
                !self.operations.iter().any(|operation| {
                    operation
                        .terminal()
                        .and_then(MaterializationTerminalV1::object_terminal_digest)
                        == Some(object.object_terminal_digest())
                })
            })
            .collect();
        if unreferenced.len() > 1 {
            return Err(ArtifactContractError::InvalidSnapshot);
        }
        if let Some(object) = unreferenced.first() {
            let last = self
                .operations
                .last()
                .ok_or(ArtifactContractError::InvalidSnapshot)?;
            if last.receipt().is_some()
                || last.terminal().is_some()
                || last.request().object_ref() != object.object_ref()
                || last.materializing().is_none()
            {
                return Err(ArtifactContractError::InvalidSnapshot);
            }
        }
        let uncertain_operations: Vec<_> = self
            .operations
            .iter()
            .filter(|operation| {
                operation.terminal().map(MaterializationTerminalV1::state)
                    == Some(MaterializationTerminalStateV1::Uncertain)
            })
            .collect();
        match self.quarantine {
            ArtifactQuarantineFactsV1::Absent => {
                if !uncertain_operations.is_empty() {
                    return Err(ArtifactContractError::InvalidQuarantineFacts);
                }
            }
            ArtifactQuarantineFactsV1::Present {
                operation_id,
                regular_file_bytes,
            } => {
                if regular_file_bytes > MAX_ARTIFACT_QUARANTINE_BYTES
                    || !self.objects.is_empty()
                    || uncertain_operations.len() != 1
                    || uncertain_operations[0].operation_id() != operation_id
                    || self
                        .operations
                        .last()
                        .map(MaterializationOperationV1::operation_id)
                        != Some(operation_id)
                    || self.objects.iter().any(|object| {
                        object.object_ref() == uncertain_operations[0].request().object_ref()
                    })
                {
                    return Err(ArtifactContractError::InvalidQuarantineFacts);
                }
            }
        }
        self.ensure_capacity()
    }

    pub fn try_successor(
        &self,
        successor: ArtifactSnapshotSuccessorV1,
    ) -> Result<Self, ArtifactContractError> {
        let mut next = self.clone();
        next.snapshot_sequence = NonZeroU64::new(
            self.snapshot_sequence
                .get()
                .checked_add(1)
                .ok_or(ArtifactContractError::ArithmeticOverflow)?,
        )
        .ok_or(ArtifactContractError::ArithmeticOverflow)?;
        if matches!(self.quarantine, ArtifactQuarantineFactsV1::Present { .. })
            && !matches!(successor, ArtifactSnapshotSuccessorV1::Receipt { .. })
        {
            return Err(ArtifactContractError::InvalidSuccessor);
        }
        match successor {
            ArtifactSnapshotSuccessorV1::Admission { request, admission } => {
                if self.operations.len() >= MAX_ARTIFACT_OPERATIONS
                    || request.config_commitment() != self.config_commitment
                    || admission.store_instance() != self.store_instance
                    || admission.operation_sequence().get()
                        != self
                            .operation_high_water
                            .checked_add(1)
                            .ok_or(ArtifactContractError::ArithmeticOverflow)?
                    || self
                        .operations
                        .iter()
                        .any(|operation| operation.operation_id() == request.operation_id())
                {
                    return Err(ArtifactContractError::InvalidSuccessor);
                }
                next.operations
                    .push(MaterializationOperationV1::admitted(request, admission)?);
                next.operation_high_water = next
                    .operation_high_water
                    .checked_add(1)
                    .ok_or(ArtifactContractError::ArithmeticOverflow)?;
            }
            ArtifactSnapshotSuccessorV1::Materializing { materializing } => {
                let operation = next
                    .operations
                    .iter_mut()
                    .find(|operation| operation.operation_id() == materializing.operation_id())
                    .ok_or(ArtifactContractError::InvalidSuccessor)?;
                if operation.materializing.is_some() || operation.terminal.is_some() {
                    return Err(ArtifactContractError::InvalidSuccessor);
                }
                operation.materializing = Some(materializing);
                operation.validate_presence()?;
            }
            ArtifactSnapshotSuccessorV1::Object {
                operation_id,
                object,
            } => {
                if self.objects.len() >= MAX_ARTIFACT_OBJECTS
                    || object.store_instance() != self.store_instance
                    || object.object_sequence().get()
                        != self
                            .object_high_water
                            .checked_add(1)
                            .ok_or(ArtifactContractError::ArithmeticOverflow)?
                    || self
                        .objects
                        .iter()
                        .any(|existing| existing.object_ref() == object.object_ref())
                {
                    return Err(ArtifactContractError::InvalidSuccessor);
                }
                let operation = self
                    .operation(operation_id)
                    .ok_or(ArtifactContractError::InvalidSuccessor)?;
                if operation.materializing().is_none()
                    || operation.terminal().is_some()
                    || operation.request().object_ref() != object.object_ref()
                {
                    return Err(ArtifactContractError::InvalidSuccessor);
                }
                next.objects.push(object);
                next.object_high_water = next
                    .object_high_water
                    .checked_add(1)
                    .ok_or(ArtifactContractError::ArithmeticOverflow)?;
            }
            ArtifactSnapshotSuccessorV1::Terminal {
                terminal,
                quarantine,
            } => {
                let operation = next
                    .operations
                    .iter_mut()
                    .find(|operation| operation.operation_id() == terminal.operation_id())
                    .ok_or(ArtifactContractError::InvalidSuccessor)?;
                if operation.terminal.is_some() || operation.receipt.is_some() {
                    return Err(ArtifactContractError::InvalidSuccessor);
                }
                operation.terminal = Some(terminal);
                operation.validate_presence()?;
                next.quarantine = quarantine;
            }
            ArtifactSnapshotSuccessorV1::Receipt { receipt } => {
                let operation = next
                    .operations
                    .iter_mut()
                    .find(|operation| operation.operation_id() == receipt.operation_id())
                    .ok_or(ArtifactContractError::InvalidSuccessor)?;
                if operation.terminal.is_none() || operation.receipt.is_some() {
                    return Err(ArtifactContractError::InvalidSuccessor);
                }
                operation.receipt = Some(receipt);
                operation.validate_presence()?;
            }
        }
        next.validate()
            .map_err(|_| ArtifactContractError::InvalidSuccessor)?;
        next.ensure_capacity()?;
        Ok(next)
    }

    #[must_use]
    pub const fn store_instance(&self) -> ArtifactStoreInstanceV1 {
        self.store_instance
    }
    #[must_use]
    pub const fn config_commitment(&self) -> ArtifactConfigCommitmentV1 {
        self.config_commitment
    }
    #[must_use]
    pub const fn snapshot_sequence(&self) -> NonZeroU64 {
        self.snapshot_sequence
    }
    #[must_use]
    pub const fn operation_high_water(&self) -> u64 {
        self.operation_high_water
    }
    #[must_use]
    pub const fn object_high_water(&self) -> u64 {
        self.object_high_water
    }
    #[must_use]
    pub fn objects(&self) -> &[ArtifactObjectRecordV1] {
        &self.objects
    }
    #[must_use]
    pub fn operations(&self) -> &[MaterializationOperationV1] {
        &self.operations
    }
    #[must_use]
    pub const fn quarantine(&self) -> ArtifactQuarantineFactsV1 {
        self.quarantine
    }

    #[must_use]
    pub fn operation(
        &self,
        operation_id: ArtifactOperationIdV1,
    ) -> Option<&MaterializationOperationV1> {
        self.operations
            .iter()
            .find(|operation| operation.operation_id() == operation_id)
    }

    pub fn recovery_start(
        &self,
        operation_id: ArtifactOperationIdV1,
    ) -> Result<ArtifactRecoveryStartV1, ArtifactContractError> {
        let operation = self
            .operation(operation_id)
            .ok_or(ArtifactContractError::InvalidReference)?;
        if operation.materializing().is_none() || operation.terminal().is_some() {
            return Err(ArtifactContractError::InvalidState);
        }
        let Some(object) = self
            .objects
            .iter()
            .find(|object| object.object_ref() == operation.request().object_ref())
            .cloned()
        else {
            return Ok(ArtifactRecoveryStartV1::NoMatchingObject);
        };
        let operation_index = self
            .operations
            .iter()
            .position(|candidate| candidate.operation_id() == operation_id)
            .ok_or(ArtifactContractError::InvalidReference)?;
        let earlier_referenced = self.operations[..operation_index].iter().any(|prior| {
            prior
                .terminal()
                .and_then(MaterializationTerminalV1::object_terminal_digest)
                == Some(object.object_terminal_digest())
        });
        if earlier_referenced {
            Ok(ArtifactRecoveryStartV1::EarlierReferenced(object))
        } else {
            Ok(ArtifactRecoveryStartV1::UnreferencedCurrent(object))
        }
    }

    pub fn verified_read_bundle(
        &self,
        receipt_ref: MaterializationReceiptRefV1,
        expected_object_ref: ArtifactObjectRefV1,
        pair: Option<VerifiedArtifactPairV1>,
    ) -> Result<VerifiedMaterializationReadBundleV1, ArtifactContractError> {
        let operation = self
            .operation(receipt_ref.operation_id())
            .ok_or(ArtifactContractError::CrossFrameMismatch)?;
        let terminal = operation
            .terminal()
            .cloned()
            .ok_or(ArtifactContractError::CrossFrameMismatch)?;
        let receipt = operation
            .receipt()
            .cloned()
            .ok_or(ArtifactContractError::CrossFrameMismatch)?;
        if receipt_ref.store_instance() != self.store_instance
            || receipt_ref.operation_sequence() != operation.admission().operation_sequence()
            || receipt_ref.operation_id() != operation.operation_id()
            || receipt_ref.receipt_digest() != receipt.receipt_digest()
            || expected_object_ref != operation.request().object_ref()
            || expected_object_ref != receipt.object_ref()
        {
            return Err(ArtifactContractError::CrossFrameMismatch);
        }
        let object = terminal
            .object_terminal_digest()
            .map(|digest| {
                self.objects
                    .iter()
                    .find(|object| object.object_terminal_digest() == digest)
                    .cloned()
                    .ok_or(ArtifactContractError::CrossFrameMismatch)
            })
            .transpose()?;
        VerifiedMaterializationReadBundleV1::verify(
            operation.request().clone(),
            operation.admission().clone(),
            operation.materializing().cloned(),
            object,
            terminal,
            receipt,
            pair,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactSnapshotSuccessorV1 {
    Admission {
        request: MaterializationRequestV1,
        admission: MaterializationAdmissionV1,
    },
    Materializing {
        materializing: MaterializingRecordV1,
    },
    Object {
        operation_id: ArtifactOperationIdV1,
        object: ArtifactObjectRecordV1,
    },
    Terminal {
        terminal: MaterializationTerminalV1,
        quarantine: ArtifactQuarantineFactsV1,
    },
    Receipt {
        receipt: MaterializationReceiptV1,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &[u8] = b"artifact-f0-prefix: ";

    fn decode_hex_fixture(input: &str) -> Vec<u8> {
        let line = input.strip_suffix('\n').expect("one trailing LF");
        assert!(!line.is_empty() && !line.contains('\r') && !line.contains('\n'));
        assert_eq!(line.len() % 2, 0);
        let mut output = Vec::with_capacity(line.len() / 2);
        let bytes = line.as_bytes();
        for pair in bytes.chunks_exact(2) {
            let decode = |byte| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                _ => panic!("fixture is not lowercase hexadecimal"),
            };
            output.push((decode(pair[0]) << 4) | decode(pair[1]));
        }
        output
    }

    fn zero_correlation_and_rehash(
        bytes: &mut [u8],
        correlation_offset: usize,
        own_digest_offset: usize,
        domain: &[u8],
    ) {
        bytes[correlation_offset..correlation_offset + 32].fill(0);
        let digest = raw_sha256(domain, &[&bytes[..own_digest_offset]]);
        bytes[own_digest_offset..own_digest_offset + 32].copy_from_slice(digest.as_bytes());
    }

    fn rehash_snapshot(bytes: &mut [u8]) {
        let body_len = read_u64(bytes, 128).expect("snapshot body length");
        let length_prefix = 160_u64.to_be_bytes();
        let body_length_prefix = body_len.to_be_bytes();
        let checksum = raw_sha256(
            SNAPSHOT_DIGEST_DOMAIN,
            &[
                &length_prefix,
                &bytes[..160],
                &body_length_prefix,
                &bytes[PXAZ_HEADER_BYTES..],
            ],
        );
        bytes[160..192].copy_from_slice(checksum.as_bytes());
    }

    fn store() -> ArtifactStoreInstanceV1 {
        ArtifactStoreInstanceV1::try_from_bytes([0xa0; 32]).expect("nonzero store")
    }

    fn config() -> ArtifactConfigCommitmentV1 {
        ArtifactConfigCommitmentV1::try_from_bytes([0xa1; 32]).expect("nonzero config")
    }

    fn operation(byte: u8) -> ArtifactOperationIdV1 {
        ArtifactOperationIdV1::try_from_bytes([byte; 16]).expect("nonzero operation")
    }

    fn pair() -> VerifiedArtifactPairV1 {
        VerifiedArtifactPairV1::from_payload(PAYLOAD).expect("canonical pair")
    }

    fn manifest_bytes() -> [u8; PXAM_BYTES] {
        ArtifactManifestV1::from_payload(PAYLOAD)
            .expect("canonical manifest")
            .encode()
    }

    fn request(operation_id: ArtifactOperationIdV1) -> MaterializationRequestV1 {
        MaterializationRequestV1::new(operation_id, config(), pair().object_ref())
    }

    fn admission(sequence: u64, request: &MaterializationRequestV1) -> MaterializationAdmissionV1 {
        MaterializationAdmissionV1::new(
            store(),
            NonZeroU64::new(sequence).expect("nonzero sequence"),
            request,
        )
    }

    fn admitted() -> ArtifactStoreSnapshotV1 {
        let request = request(operation(0xa2));
        let admission = admission(1, &request);
        ArtifactStoreSnapshotV1::initial(store(), config(), request, admission)
            .expect("initial snapshot")
    }

    fn receipt_ref(
        snapshot: &ArtifactStoreSnapshotV1,
        operation_id: ArtifactOperationIdV1,
    ) -> MaterializationReceiptRefV1 {
        MaterializationReceiptRefV1::from_receipt(
            snapshot
                .operation(operation_id)
                .and_then(MaterializationOperationV1::receipt)
                .expect("terminal receipt"),
        )
    }

    fn materialized() -> (
        ArtifactStoreSnapshotV1,
        VerifiedArtifactPairV1,
        MaterializationTerminalV1,
        MaterializationReceiptV1,
    ) {
        let pair = pair();
        let initial = admitted();
        let operation = initial.operations()[0].clone();
        let materializing = MaterializingRecordV1::new(operation.admission());
        let progressing = initial
            .try_successor(ArtifactSnapshotSuccessorV1::Materializing {
                materializing: materializing.clone(),
            })
            .expect("materializing successor");
        let object = ArtifactObjectRecordV1::new(
            store(),
            NonZeroU64::new(1).expect("nonzero object sequence"),
            &pair,
        );
        let object_terminal = progressing
            .try_successor(ArtifactSnapshotSuccessorV1::Object {
                operation_id: operation.operation_id(),
                object: object.clone(),
            })
            .expect("object successor");
        let terminal = MaterializationTerminalV1::new(
            operation.admission(),
            Some(&materializing),
            Some(&object),
            MaterializationTerminalStateV1::Materialized,
        )
        .expect("materialized terminal");
        let terminal_snapshot = object_terminal
            .try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: terminal.clone(),
                quarantine: ArtifactQuarantineFactsV1::Absent,
            })
            .expect("terminal successor");
        let receipt = MaterializationReceiptV1::new(&terminal);
        let receipt_snapshot = terminal_snapshot
            .try_successor(ArtifactSnapshotSuccessorV1::Receipt {
                receipt: receipt.clone(),
            })
            .expect("receipt successor");
        (receipt_snapshot, pair, terminal, receipt)
    }

    #[test]
    fn manifest_profile_classification_matches_canonical_profile() {
        assert_eq!(
            ArtifactManifestV1::classify_profile(&manifest_bytes()),
            ArtifactManifestProfileClassificationV1::Match,
        );
    }

    #[test]
    fn manifest_profile_classification_rejects_declared_length_mismatch() {
        let mut bytes = manifest_bytes();
        put_u16(&mut bytes, 12, PROFILE.len() as u16 + 1);
        assert_eq!(
            ArtifactManifestV1::classify_profile(&bytes),
            ArtifactManifestProfileClassificationV1::Mismatch,
        );
    }

    #[test]
    fn manifest_profile_classification_rejects_literal_mismatch() {
        let mut bytes = manifest_bytes();
        bytes[80] ^= 1;
        assert_eq!(
            ArtifactManifestV1::classify_profile(&bytes),
            ArtifactManifestProfileClassificationV1::Mismatch,
        );
    }

    #[test]
    fn manifest_profile_mismatch_precedes_other_manifest_corruption() {
        let mut bytes = manifest_bytes();
        bytes[0] ^= 1;
        bytes[80] ^= 1;
        assert_eq!(
            ArtifactManifestV1::classify_profile(&bytes),
            ArtifactManifestProfileClassificationV1::Mismatch,
        );
        assert_eq!(
            ArtifactManifestV1::decode(&bytes),
            Err(ArtifactContractError::InvalidMagic),
        );
    }

    #[test]
    fn short_manifest_profile_classification_is_indeterminate() {
        let bytes = manifest_bytes();
        assert_eq!(
            ArtifactManifestV1::classify_profile(&bytes[..PXAM_BYTES - 1]),
            ArtifactManifestProfileClassificationV1::Indeterminate,
        );
    }

    #[test]
    fn payload_contract_is_shared_by_manifest_and_pair() {
        assert!(ArtifactManifestV1::from_payload(PAYLOAD).is_ok());
        for invalid in [
            b"bad\0 ".as_slice(),
            b"bad\n ".as_slice(),
            b"bad\x7f ".as_slice(),
            b"bad\x80 ".as_slice(),
            b"missing-space".as_slice(),
        ] {
            assert_eq!(
                ArtifactManifestV1::from_payload(invalid),
                Err(ArtifactContractError::InvalidPayloadLength)
            );
        }
        let manifest = ArtifactManifestV1::from_payload(PAYLOAD).expect("manifest");
        assert_eq!(ArtifactManifestV1::decode(&manifest.encode()), Ok(manifest));
        let mut zero_digest_manifest = manifest.encode();
        zero_digest_manifest[32..64].fill(0);
        assert_eq!(
            ArtifactManifestV1::decode(&zero_digest_manifest),
            Err(ArtifactContractError::DigestMismatch)
        );
        let mut trailing = manifest.encode().to_vec();
        trailing.push(0);
        assert_eq!(
            ArtifactManifestV1::decode(&trailing),
            Err(ArtifactContractError::InvalidLength)
        );
    }

    #[test]
    fn binary_and_text_references_are_canonical_and_nonzero() {
        let pair = pair();
        let object_ref = pair.object_ref();
        assert_eq!(
            ArtifactObjectRefV1::decode(&object_ref.encode()),
            Ok(object_ref)
        );
        assert_eq!(object_ref.to_string().parse(), Ok(object_ref));
        assert_eq!(
            ArtifactObjectRefV1::try_new(
                Digest32::from_bytes([0; 32]),
                object_ref.manifest_digest()
            ),
            Err(ArtifactContractError::DigestMismatch)
        );
        assert_eq!(
            ArtifactObjectRefV1::try_new(
                object_ref.payload_digest(),
                Digest32::from_bytes([0; 32])
            ),
            Err(ArtifactContractError::DigestMismatch)
        );
        let zero_payload_ref = format!("sha256:{}:{}", "00".repeat(32), "11".repeat(32));
        assert_eq!(
            zero_payload_ref.parse::<ArtifactObjectRefV1>(),
            Err(ArtifactContractError::DigestMismatch)
        );
        let (snapshot, _, _, receipt) = materialized();
        let reference = MaterializationReceiptRefV1::from_receipt(&receipt);
        assert_eq!(reference.to_string().parse(), Ok(reference));
        let zero_receipt = format!(
            "pxamr1:{}:1:{}:{}",
            "a0".repeat(32),
            "a2".repeat(16),
            "00".repeat(32)
        );
        assert_eq!(
            zero_receipt.parse::<MaterializationReceiptRefV1>(),
            Err(ArtifactContractError::DigestMismatch)
        );
        assert_eq!(snapshot.operation_high_water(), 1);
    }

    #[test]
    fn fixed_frames_reject_trailing_and_digest_drift() {
        let pair = pair();
        let request = request(operation(0xa2));
        let admission = admission(1, &request);
        let materializing = MaterializingRecordV1::new(&admission);
        let object =
            ArtifactObjectRecordV1::new(store(), NonZeroU64::new(1).expect("sequence"), &pair);
        let terminal = MaterializationTerminalV1::new(
            &admission,
            Some(&materializing),
            Some(&object),
            MaterializationTerminalStateV1::Materialized,
        )
        .expect("terminal");
        let receipt = MaterializationReceiptV1::new(&terminal);
        let admitted_operation =
            MaterializationOperationV1::admitted(request.clone(), admission.clone())
                .expect("admitted operation");
        let operation_wire = admitted_operation.encode_canonical().expect("PXOP wire");
        assert_eq!(operation_wire.len(), 416);
        assert_eq!(
            MaterializationOperationV1::decode_canonical(&operation_wire),
            Ok(admitted_operation)
        );
        assert_eq!(
            MaterializationRequestV1::decode(&request.encode()),
            Ok(request.clone())
        );
        assert_eq!(
            MaterializationAdmissionV1::decode(&admission.encode()),
            Ok(admission.clone())
        );
        assert_eq!(
            MaterializingRecordV1::decode(&materializing.encode()),
            Ok(materializing.clone())
        );
        assert_eq!(
            ArtifactObjectRecordV1::decode(&object.encode()),
            Ok(object.clone())
        );
        assert_eq!(
            MaterializationTerminalV1::decode(&terminal.encode()),
            Ok(terminal.clone())
        );
        assert_eq!(
            MaterializationReceiptV1::decode(&receipt.encode()),
            Ok(receipt)
        );
        let mut corrupt = request.encode().to_vec();
        corrupt[50] ^= 1;
        assert_eq!(
            MaterializationRequestV1::decode(&corrupt),
            Err(ArtifactContractError::DigestMismatch)
        );
        corrupt = request.encode().to_vec();
        corrupt.push(0);
        assert_eq!(
            MaterializationRequestV1::decode(&corrupt),
            Err(ArtifactContractError::InvalidLength)
        );
    }

    #[test]
    fn verified_read_bundle_requires_exact_receipt_and_object_locators() {
        let (snapshot, pair, _, receipt) = materialized();
        let reference = MaterializationReceiptRefV1::from_receipt(&receipt);
        assert!(
            snapshot
                .verified_read_bundle(reference, pair.object_ref(), Some(pair.clone()))
                .is_ok()
        );

        let mut store_drift = reference;
        store_drift.store_instance =
            ArtifactStoreInstanceV1::try_from_bytes([0xb0; 32]).expect("different store");
        let mut sequence_drift = reference;
        sequence_drift.operation_sequence = NonZeroU64::new(2).expect("different sequence");
        let mut operation_drift = reference;
        operation_drift.operation_id = operation(0xb1);
        let mut receipt_drift = reference;
        receipt_drift.receipt_digest = Digest32::from_bytes([0xb2; 32]);
        for drift in [store_drift, sequence_drift, operation_drift, receipt_drift] {
            assert_eq!(
                snapshot.verified_read_bundle(drift, pair.object_ref(), Some(pair.clone())),
                Err(ArtifactContractError::CrossFrameMismatch)
            );
        }

        let other_pair = VerifiedArtifactPairV1::from_payload(b"other-prefix: ")
            .expect("different canonical pair");
        assert_eq!(
            snapshot.verified_read_bundle(reference, other_pair.object_ref(), Some(pair)),
            Err(ArtifactContractError::CrossFrameMismatch)
        );
    }

    #[test]
    fn standalone_frames_reject_zero_correlation_with_recomputed_own_digest() {
        let pair = pair();
        let request = request(operation(0xa2));
        let admission = admission(1, &request);
        let materializing = MaterializingRecordV1::new(&admission);
        let object = ArtifactObjectRecordV1::new(
            store(),
            NonZeroU64::new(1).expect("object sequence"),
            &pair,
        );
        let terminal = MaterializationTerminalV1::new(
            &admission,
            Some(&materializing),
            Some(&object),
            MaterializationTerminalStateV1::Materialized,
        )
        .expect("terminal");
        let receipt = MaterializationReceiptV1::new(&terminal);

        let mut pxaa = admission.encode();
        zero_correlation_and_rehash(&mut pxaa, 72, 176, ADMISSION_DIGEST_DOMAIN);
        assert_eq!(
            MaterializationAdmissionV1::decode(&pxaa),
            Err(ArtifactContractError::DigestMismatch)
        );

        for offset in [72, 104] {
            let mut pxmu = materializing.encode();
            zero_correlation_and_rehash(&mut pxmu, offset, 208, MATERIALIZING_DIGEST_DOMAIN);
            assert_eq!(
                MaterializingRecordV1::decode(&pxmu),
                Err(ArtifactContractError::DigestMismatch)
            );

            let mut pxaw = terminal.encode();
            zero_correlation_and_rehash(&mut pxaw, offset, 272, OPERATION_TERMINAL_DIGEST_DOMAIN);
            assert_eq!(
                MaterializationTerminalV1::decode(&pxaw),
                Err(ArtifactContractError::DigestMismatch)
            );

            let mut pxax = receipt.encode();
            zero_correlation_and_rehash(&mut pxax, offset, 208, RECEIPT_DIGEST_DOMAIN);
            assert_eq!(
                MaterializationReceiptV1::decode(&pxax),
                Err(ArtifactContractError::DigestMismatch)
            );
        }
    }

    #[test]
    fn five_successors_have_frozen_frame_and_accounting_sizes() {
        let initial = admitted();
        assert_eq!(initial.encode_canonical().expect("encode").len(), 672);
        assert_eq!(initial.accounted_rest_bytes(), Ok(672));
        let operation = initial.operations()[0].clone();
        let materializing = MaterializingRecordV1::new(operation.admission());
        let progressing = initial
            .try_successor(ArtifactSnapshotSuccessorV1::Materializing {
                materializing: materializing.clone(),
            })
            .expect("progressing");
        assert_eq!(progressing.encode_canonical().expect("encode").len(), 912);
        assert_eq!(
            progressing.recovery_start(operation.operation_id()),
            Ok(ArtifactRecoveryStartV1::NoMatchingObject)
        );
        let progressing_wire = progressing.encode_canonical().expect("wire");
        let progressing_candidate =
            ArtifactStoreSnapshotCandidateV1::decode_canonical(&progressing_wire)
                .expect("candidate");
        let progressing_claim = ArtifactFilesystemClaimV1::Materializing {
            operation_id: operation.operation_id(),
            object_ref: operation.request().object_ref(),
        };
        assert_eq!(progressing_candidate.filesystem_claim(), &progressing_claim);
        assert_eq!(
            progressing_candidate.validate_filesystem(progressing_claim),
            Ok(progressing.clone())
        );
        let pair = pair();
        let object =
            ArtifactObjectRecordV1::new(store(), NonZeroU64::new(1).expect("sequence"), &pair);
        let object_snapshot = progressing
            .try_successor(ArtifactSnapshotSuccessorV1::Object {
                operation_id: operation.operation_id(),
                object: object.clone(),
            })
            .expect("object");
        assert_eq!(
            object_snapshot.encode_canonical().expect("encode").len(),
            1104
        );
        assert_eq!(object_snapshot.accounted_rest_bytes(), Ok(1330));
        assert_eq!(
            object_snapshot.recovery_start(operation.operation_id()),
            Ok(ArtifactRecoveryStartV1::UnreferencedCurrent(object.clone()))
        );
        let invalid_e = MaterializationTerminalV1::new(
            operation.admission(),
            Some(&materializing),
            Some(&object),
            MaterializationTerminalStateV1::AlreadyMaterialized,
        )
        .expect("frame-local E terminal");
        assert_eq!(
            object_snapshot.try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: invalid_e,
                quarantine: ArtifactQuarantineFactsV1::Absent,
            }),
            Err(ArtifactContractError::InvalidSuccessor)
        );
        let terminal = MaterializationTerminalV1::new(
            operation.admission(),
            Some(&materializing),
            Some(&object),
            MaterializationTerminalStateV1::Materialized,
        )
        .expect("terminal");
        let terminal_snapshot = object_snapshot
            .try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: terminal.clone(),
                quarantine: ArtifactQuarantineFactsV1::Absent,
            })
            .expect("terminal successor");
        assert_eq!(
            terminal_snapshot.encode_canonical().expect("encode").len(),
            1408
        );
        assert_eq!(terminal_snapshot.accounted_rest_bytes(), Ok(1634));
        let final_snapshot = terminal_snapshot
            .try_successor(ArtifactSnapshotSuccessorV1::Receipt {
                receipt: MaterializationReceiptV1::new(&terminal),
            })
            .expect("receipt");
        let wire = final_snapshot.encode_canonical().expect("wire");
        assert_eq!(wire.len(), 1648);
        assert_eq!(final_snapshot.accounted_rest_bytes(), Ok(1874));
        assert_eq!(
            ArtifactStoreSnapshotCandidateV1::decode_canonical(&wire).and_then(|candidate| {
                candidate.validate_filesystem(ArtifactFilesystemClaimV1::Stable)
            }),
            Ok(final_snapshot)
        );
    }

    #[test]
    fn m_and_e_are_selected_only_by_strict_snapshot_history() {
        let (materialized, pair, _, _) = materialized();
        let second_request = request(operation(0xa3));
        let second_admission = admission(2, &second_request);
        let second = materialized
            .try_successor(ArtifactSnapshotSuccessorV1::Admission {
                request: second_request,
                admission: second_admission.clone(),
            })
            .expect("second admission");
        let materializing = MaterializingRecordV1::new(&second_admission);
        let second = second
            .try_successor(ArtifactSnapshotSuccessorV1::Materializing {
                materializing: materializing.clone(),
            })
            .expect("second materializing");
        assert_eq!(second.encode_canonical().expect("wire").len(), 2304);
        assert_eq!(second.accounted_rest_bytes(), Ok(2530));
        let indexed = second.objects()[0].clone();
        assert_eq!(
            second.recovery_start(operation(0xa3)),
            Ok(ArtifactRecoveryStartV1::EarlierReferenced(indexed.clone()))
        );
        let failed = MaterializationTerminalV1::new(
            &second_admission,
            Some(&materializing),
            None,
            MaterializationTerminalStateV1::Failed,
        )
        .expect("frame-local failed terminal");
        assert_eq!(
            second.try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: failed,
                quarantine: ArtifactQuarantineFactsV1::Absent,
            }),
            Err(ArtifactContractError::InvalidSuccessor)
        );
        let terminal = MaterializationTerminalV1::new(
            &second_admission,
            Some(&materializing),
            Some(&indexed),
            MaterializationTerminalStateV1::AlreadyMaterialized,
        )
        .expect("already materialized terminal");
        let terminal_snapshot = second
            .try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: terminal.clone(),
                quarantine: ArtifactQuarantineFactsV1::Absent,
            })
            .expect("E terminal");
        assert_eq!(
            terminal_snapshot.encode_canonical().expect("wire").len(),
            2608
        );
        assert_eq!(terminal_snapshot.accounted_rest_bytes(), Ok(2834));
        let final_snapshot = terminal_snapshot
            .try_successor(ArtifactSnapshotSuccessorV1::Receipt {
                receipt: MaterializationReceiptV1::new(&terminal),
            })
            .expect("E receipt");
        assert_eq!(final_snapshot.encode_canonical().expect("wire").len(), 2848);
        assert_eq!(final_snapshot.accounted_rest_bytes(), Ok(3074));
        let reference = receipt_ref(&final_snapshot, operation(0xa3));
        assert!(
            final_snapshot
                .verified_read_bundle(reference, pair.object_ref(), Some(pair))
                .is_ok()
        );
    }

    #[test]
    fn quarantine_rejects_every_historical_pxav() {
        let (snapshot, _, _, _) = materialized();
        let second_pair =
            VerifiedArtifactPairV1::from_payload(b"other-prefix: ").expect("second canonical pair");
        let second_request =
            MaterializationRequestV1::new(operation(0xa3), config(), second_pair.object_ref());
        let second_admission = admission(2, &second_request);
        let admitted = snapshot
            .try_successor(ArtifactSnapshotSuccessorV1::Admission {
                request: second_request,
                admission: second_admission.clone(),
            })
            .expect("second admission");
        let materializing = MaterializingRecordV1::new(&second_admission);
        let progressing = admitted
            .try_successor(ArtifactSnapshotSuccessorV1::Materializing {
                materializing: materializing.clone(),
            })
            .expect("second materializing");
        let uncertain = MaterializationTerminalV1::new(
            &second_admission,
            Some(&materializing),
            None,
            MaterializationTerminalStateV1::Uncertain,
        )
        .expect("frame-local uncertain terminal");
        assert_eq!(
            progressing.try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: uncertain,
                quarantine: ArtifactQuarantineFactsV1::Present {
                    operation_id: operation(0xa3),
                    regular_file_bytes: 0,
                },
            }),
            Err(ArtifactContractError::InvalidSuccessor)
        );
    }

    #[test]
    fn failed_and_uncertain_have_distinct_conditional_chains() {
        let initial = admitted();
        let admission = initial.operations()[0].admission().clone();
        let failed = MaterializationTerminalV1::new(
            &admission,
            None,
            None,
            MaterializationTerminalStateV1::Failed,
        )
        .expect("failed terminal");
        let failed_snapshot = initial
            .try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: failed.clone(),
                quarantine: ArtifactQuarantineFactsV1::Absent,
            })
            .expect("failed successor");
        assert_eq!(failed_snapshot.encode_canonical().expect("wire").len(), 976);
        let failed_snapshot = failed_snapshot
            .try_successor(ArtifactSnapshotSuccessorV1::Receipt {
                receipt: MaterializationReceiptV1::new(&failed),
            })
            .expect("failed receipt");
        assert_eq!(
            failed_snapshot.encode_canonical().expect("wire").len(),
            1216
        );
        let failed_ref = receipt_ref(&failed_snapshot, operation(0xa2));
        let failed_object_ref = failed_snapshot.operations()[0].request().object_ref();
        assert!(
            failed_snapshot
                .verified_read_bundle(failed_ref, failed_object_ref, None)
                .is_ok()
        );

        let progressing = admitted();
        let admission = progressing.operations()[0].admission().clone();
        let materializing = MaterializingRecordV1::new(&admission);
        let progressing = progressing
            .try_successor(ArtifactSnapshotSuccessorV1::Materializing {
                materializing: materializing.clone(),
            })
            .expect("materializing");
        let uncertain = MaterializationTerminalV1::new(
            &admission,
            Some(&materializing),
            None,
            MaterializationTerminalStateV1::Uncertain,
        )
        .expect("uncertain terminal");
        assert_eq!(
            progressing.try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: uncertain.clone(),
                quarantine: ArtifactQuarantineFactsV1::Absent,
            }),
            Err(ArtifactContractError::InvalidSuccessor)
        );
        let quarantine = ArtifactQuarantineFactsV1::Present {
            operation_id: operation(0xa2),
            regular_file_bytes: 0,
        };
        let uncertain_snapshot = progressing
            .try_successor(ArtifactSnapshotSuccessorV1::Terminal {
                terminal: uncertain.clone(),
                quarantine,
            })
            .expect("uncertain successor");
        assert_eq!(
            uncertain_snapshot.encode_canonical().expect("wire").len(),
            1216
        );
        let wire = uncertain_snapshot.encode_canonical().expect("wire");
        assert_eq!(
            ArtifactStoreSnapshotCandidateV1::decode_canonical(&wire).and_then(|candidate| {
                candidate.validate_filesystem(ArtifactFilesystemClaimV1::Stable)
            }),
            Err(ArtifactContractError::InvalidQuarantineFacts)
        );
        assert_eq!(
            ArtifactStoreSnapshotCandidateV1::decode_canonical(&wire).and_then(|candidate| {
                candidate.validate_filesystem(ArtifactFilesystemClaimV1::Quarantined {
                    operation_id: operation(0xa2),
                    object_ref: pair().object_ref(),
                    regular_file_bytes: 0,
                })
            }),
            Ok(uncertain_snapshot.clone())
        );
        let uncertain_snapshot = uncertain_snapshot
            .try_successor(ArtifactSnapshotSuccessorV1::Receipt {
                receipt: MaterializationReceiptV1::new(&uncertain),
            })
            .expect("uncertain receipt");
        assert_eq!(
            uncertain_snapshot.encode_canonical().expect("wire").len(),
            1456
        );
        let uncertain_ref = receipt_ref(&uncertain_snapshot, operation(0xa2));
        let uncertain_object_ref = uncertain_snapshot.operations()[0].request().object_ref();
        assert!(
            uncertain_snapshot
                .verified_read_bundle(uncertain_ref, uncertain_object_ref, None)
                .is_ok()
        );
        assert_eq!(
            uncertain_snapshot.verified_read_bundle(
                uncertain_ref,
                uncertain_object_ref,
                Some(pair()),
            ),
            Err(ArtifactContractError::CrossFrameMismatch)
        );
    }

    #[test]
    fn snapshot_checksum_exact_eof_and_capacity_components_fail_closed() {
        let snapshot = admitted();
        let mut sequence_drift = snapshot.encode_canonical().expect("wire").into_vec();
        put_u64(&mut sequence_drift, 96, 2);
        rehash_snapshot(&mut sequence_drift);
        assert_eq!(
            ArtifactStoreSnapshotCandidateV1::decode_canonical(&sequence_drift),
            Err(ArtifactContractError::InvalidSnapshot)
        );
        let mut wire = snapshot.encode_canonical().expect("wire").into_vec();
        wire[160] ^= 1;
        assert_eq!(
            ArtifactStoreSnapshotCandidateV1::decode_canonical(&wire).and_then(|candidate| {
                candidate.validate_filesystem(ArtifactFilesystemClaimV1::Stable)
            }),
            Err(ArtifactContractError::DigestMismatch)
        );
        let mut trailing = snapshot.encode_canonical().expect("wire").into_vec();
        trailing.push(0);
        assert_eq!(
            ArtifactStoreSnapshotCandidateV1::decode_canonical(&trailing).and_then(|candidate| {
                candidate.validate_filesystem(ArtifactFilesystemClaimV1::Stable)
            }),
            Err(ArtifactContractError::InvalidHeader)
        );
        assert_eq!(
            ArtifactCapacityInputV1::new(
                ARTIFACT_DEFENSE_CEILING_BYTES - MAX_ARTIFACT_SNAPSHOT_BYTES as u64,
                MAX_ARTIFACT_SNAPSHOT_BYTES as u64,
                0,
                0,
                MAX_ARTIFACT_OBJECTS,
                MAX_ARTIFACT_OPERATIONS,
                MAX_ARTIFACT_QUARANTINE_BYTES,
            )
            .checked_total(),
            Ok(ARTIFACT_DEFENSE_CEILING_BYTES)
        );
        assert_eq!(
            ArtifactCapacityInputV1::new(
                0,
                MAX_ARTIFACT_SNAPSHOT_BYTES as u64 + 1,
                0,
                0,
                MAX_ARTIFACT_OBJECTS,
                MAX_ARTIFACT_OPERATIONS,
                MAX_ARTIFACT_QUARANTINE_BYTES,
            )
            .checked_total(),
            Err(ArtifactContractError::CapacityExceeded)
        );
        for input in [
            ArtifactCapacityInputV1::new(
                0,
                0,
                0,
                0,
                MAX_ARTIFACT_OBJECTS + 1,
                MAX_ARTIFACT_OPERATIONS,
                MAX_ARTIFACT_QUARANTINE_BYTES,
            ),
            ArtifactCapacityInputV1::new(
                0,
                0,
                0,
                0,
                MAX_ARTIFACT_OBJECTS,
                MAX_ARTIFACT_OPERATIONS + 1,
                MAX_ARTIFACT_QUARANTINE_BYTES,
            ),
            ArtifactCapacityInputV1::new(
                0,
                0,
                0,
                0,
                MAX_ARTIFACT_OBJECTS,
                MAX_ARTIFACT_OPERATIONS,
                MAX_ARTIFACT_QUARANTINE_BYTES + 1,
            ),
        ] {
            assert_eq!(
                input.checked_total(),
                Err(ArtifactContractError::CapacityExceeded)
            );
        }
        assert_eq!(
            ArtifactCapacityInputV1::new(u64::MAX, 1, 0, 0, 0, 0, 0,).checked_total(),
            Err(ArtifactContractError::ArithmeticOverflow)
        );
    }

    #[test]
    fn hardcoded_core_and_snapshot_fixtures_are_consumed_without_encoder_oracles() {
        let pxam = decode_hex_fixture(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxam_v1.hex"
        ));
        let manifest = ArtifactManifestV1::decode(&pxam).expect("hardcoded PXAM");
        assert_eq!(manifest.encode().as_slice(), pxam);

        let pxak = decode_hex_fixture(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxak_v1.hex"
        ));
        let object_ref = ArtifactObjectRefV1::decode(&pxak).expect("hardcoded PXAK");
        assert_eq!(object_ref.encode().as_slice(), pxak);

        let pxaq = decode_hex_fixture(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxaq_v1.hex"
        ));
        let request = MaterializationRequestV1::decode(&pxaq).expect("hardcoded PXAQ");
        assert_eq!(request.encode().as_slice(), pxaq);

        let pxaa = decode_hex_fixture(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxaa_v1.hex"
        ));
        let admission = MaterializationAdmissionV1::decode(&pxaa).expect("hardcoded PXAA");
        assert_eq!(admission.encode().as_slice(), pxaa);

        let pxmu = decode_hex_fixture(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxmu_v1.hex"
        ));
        let materializing = MaterializingRecordV1::decode(&pxmu).expect("hardcoded PXMU");
        assert_eq!(materializing.encode().as_slice(), pxmu);

        let pxav = decode_hex_fixture(include_str!(
            "../../../tests/fixtures/wire/artifact_f0_pxav_v1.hex"
        ));
        let object = ArtifactObjectRecordV1::decode(&pxav).expect("hardcoded PXAV");
        assert_eq!(object.encode().as_slice(), pxav);

        for fixture in [
            include_str!("../../../tests/fixtures/wire/artifact_f0_pxaw_materialized_v1.hex"),
            include_str!(
                "../../../tests/fixtures/wire/artifact_f0_pxaw_already_materialized_v1.hex"
            ),
            include_str!("../../../tests/fixtures/wire/artifact_f0_pxaw_failed_v1.hex"),
            include_str!("../../../tests/fixtures/wire/artifact_f0_pxaw_uncertain_v1.hex"),
        ] {
            let bytes = decode_hex_fixture(fixture);
            let value = MaterializationTerminalV1::decode(&bytes).expect("hardcoded PXAW");
            assert_eq!(value.encode().as_slice(), bytes);
        }
        for fixture in [
            include_str!("../../../tests/fixtures/wire/artifact_f0_pxax_materialized_v1.hex"),
            include_str!(
                "../../../tests/fixtures/wire/artifact_f0_pxax_already_materialized_v1.hex"
            ),
            include_str!("../../../tests/fixtures/wire/artifact_f0_pxax_failed_v1.hex"),
            include_str!("../../../tests/fixtures/wire/artifact_f0_pxax_uncertain_v1.hex"),
        ] {
            let bytes = decode_hex_fixture(fixture);
            let value = MaterializationReceiptV1::decode(&bytes).expect("hardcoded PXAX");
            assert_eq!(value.encode().as_slice(), bytes);
        }

        let operation_a2 =
            ArtifactOperationIdV1::try_from_bytes([0xa2; 16]).expect("ledger operation one");
        let operation_a3 =
            ArtifactOperationIdV1::try_from_bytes([0xa3; 16]).expect("ledger operation two");
        let materializing_a2 = ArtifactFilesystemClaimV1::Materializing {
            operation_id: operation_a2,
            object_ref,
        };
        let materializing_a3 = ArtifactFilesystemClaimV1::Materializing {
            operation_id: operation_a3,
            object_ref,
        };
        let quarantined = ArtifactFilesystemClaimV1::Quarantined {
            operation_id: operation_a2,
            object_ref,
            regular_file_bytes: 0,
        };
        for (fixture, facts) in [
            (
                include_str!("../../../tests/fixtures/wire/artifact_f0_pxaz_admitted_v1.hex"),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!("../../../tests/fixtures/wire/artifact_f0_pxaz_materializing_v1.hex"),
                materializing_a2.clone(),
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_object_terminal_v1.hex"
                ),
                materializing_a2,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_materialized_terminal_v1.hex"
                ),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_materialized_receipt_v1.hex"
                ),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_failed_terminal_v1.hex"
                ),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!("../../../tests/fixtures/wire/artifact_f0_pxaz_failed_receipt_v1.hex"),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_failed_after_materializing_terminal_v1.hex"
                ),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_failed_after_materializing_receipt_v1.hex"
                ),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_already_materialized_materializing_v1.hex"
                ),
                materializing_a3,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_already_materialized_terminal_v1.hex"
                ),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_already_materialized_receipt_v1.hex"
                ),
                ArtifactFilesystemClaimV1::Stable,
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_uncertain_blocked_v1.hex"
                ),
                quarantined.clone(),
            ),
            (
                include_str!(
                    "../../../tests/fixtures/wire/artifact_f0_pxaz_uncertain_receipt_blocked_v1.hex"
                ),
                quarantined,
            ),
        ] {
            let bytes = decode_hex_fixture(fixture);
            let candidate = ArtifactStoreSnapshotCandidateV1::decode_canonical(&bytes)
                .expect("hardcoded PXAZ candidate");
            assert_eq!(candidate.filesystem_claim(), &facts);
            let snapshot = candidate
                .validate_filesystem(facts)
                .expect("hardcoded PXAZ filesystem facts");
            assert_eq!(
                snapshot
                    .encode_canonical()
                    .expect("canonical PXAZ")
                    .as_ref(),
                bytes.as_slice()
            );
        }
    }
}
