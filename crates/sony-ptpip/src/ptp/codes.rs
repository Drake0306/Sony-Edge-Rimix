// Operations
pub const OP_GET_DEVICE_INFO: u16 = 0x1001;
pub const OP_OPEN_SESSION: u16 = 0x1002;
pub const OP_CLOSE_SESSION: u16 = 0x1003;
pub const OP_GET_STORAGE_IDS: u16 = 0x1004;
pub const OP_GET_STORAGE_INFO: u16 = 0x1005;
pub const OP_GET_OBJECT_HANDLES: u16 = 0x1007;
pub const OP_GET_OBJECT_INFO: u16 = 0x1008;
pub const OP_GET_OBJECT: u16 = 0x1009;
pub const OP_GET_PARTIAL_OBJECT: u16 = 0x101B;
pub const OP_SDIO_CONNECT: u16 = 0x9201;
pub const OP_SDIO_GET_EXT_DEVICE_INFO: u16 = 0x9202;
pub const OP_SDIO_SET_EXT_DEVICE_PROP_VALUE: u16 = 0x9205;
pub const OP_SDIO_CONTROL_DEVICE: u16 = 0x9207;
pub const OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO: u16 = 0x9209;
pub const OP_SDIO_GET_EXT_DEVICE_PROP: u16 = 0x9251;

// Response codes
pub const RC_OK: u16 = 0x2001;
pub const RC_OPERATION_NOT_SUPPORTED: u16 = 0x2005;
pub const RC_INVALID_OBJECT_HANDLE: u16 = 0x2009;
pub const RC_ACCESS_DENIED: u16 = 0x200F;
pub const RC_DEVICE_BUSY: u16 = 0x2019;
pub const RC_INVALID_PARAMETER: u16 = 0x201D;

// Events
pub const EV_SONY_OBJECT_ADDED: u16 = 0xC201;
pub const EV_SONY_DEVICE_PROP_CHANGED: u16 = 0xC203;
pub const EV_SONY_CAPTURED: u16 = 0xC206;

// Object handles
pub const HANDLE_LIVE_VIEW: u32 = 0xFFFF_C002;
pub const HANDLE_CAPTURED_STILL: u32 = 0xFFFF_C001;

// Object formats
pub const FMT_ASSOCIATION: u16 = 0x3001;
pub const FMT_JPEG: u16 = 0x3801;
pub const FMT_SONY_RAW: u16 = 0xB101;
pub const FMT_MP4: u16 = 0xB984;

// Device properties
pub const PROP_WHITE_BALANCE: u16 = 0x5005;
pub const PROP_F_NUMBER: u16 = 0x5007;
pub const PROP_FOCUS_MODE: u16 = 0x500A;
pub const PROP_EXPOSURE_PROGRAM: u16 = 0x500E;
pub const PROP_EXPOSURE_BIAS: u16 = 0x5010;
pub const PROP_STILL_CAPTURE_MODE: u16 = 0x5013;
pub const PROP_COMPRESSION: u16 = 0x5004;
pub const PROP_SHUTTER_SPEED: u16 = 0xD20D;
pub const PROP_ISO: u16 = 0xD21E;
pub const PROP_CAPTURED_READY: u16 = 0xD215;
pub const PROP_BATTERY: u16 = 0xD218;
pub const PROP_MOVIE_REC_STATE: u16 = 0xD21D;
pub const PROP_LIVE_VIEW_STATUS: u16 = 0xD221;
pub const PROP_STILL_DESTINATION: u16 = 0xD222;
pub const PROP_FOCUS_FOUND: u16 = 0xD213;
pub const PROP_IMAGE_SIZE_TO_HOST: u16 = 0xD268;
/// File format on bodies from about 2020 on (RAW, RAW+JPEG, JPEG), where
/// older bodies use the standard 0x5004. libgphoto2: CompressionSetting.
pub const PROP_FILE_FORMAT: u16 = 0xD253;
pub const PROP_JPEG_QUALITY: u16 = 0xD252;
/// Which files of a RAW+JPEG pair are sent to the computer.
pub const PROP_PC_SAVE_FORMAT: u16 = 0xD269;

// Controls
pub const CTRL_SHUTTER_HALF: u16 = 0xD2C1;
pub const CTRL_SHUTTER_FULL: u16 = 0xD2C2;
pub const CTRL_MOVIE_REC: u16 = 0xD2C8;
/// Where the autofocus frame goes: a u32, x in the upper 16 bits and y in
/// the lower, on a 640 by 480 grid (libgphoto2: AFAreaPosition).
pub const CTRL_AF_AREA_POSITION: u16 = 0xD2DC;
/// A touch on the camera's screen, sent from the computer: where, as
/// [`CTRL_AF_AREA_POSITION`]. Bodies that have it list it among their controls.
pub const CTRL_REMOTE_TOUCH: u16 = 0xD2E4;
/// Which part of the picture autofocus uses (wide, zone, a movable spot...).
pub const PROP_FOCUS_AREA: u16 = 0xD22C;
/// Shots left on the card in slot 1.
pub const PROP_SHOTS_LEFT: u16 = 0xD249;

// Datatypes
pub const DT_INT8: u16 = 0x0001;
pub const DT_UINT8: u16 = 0x0002;
pub const DT_INT16: u16 = 0x0003;
pub const DT_UINT16: u16 = 0x0004;
pub const DT_INT32: u16 = 0x0005;
pub const DT_UINT32: u16 = 0x0006;
pub const DT_INT64: u16 = 0x0007;
pub const DT_UINT64: u16 = 0x0008;
pub const DT_STR: u16 = 0xFFFF;
pub const DT_ARRAY_FLAG: u16 = 0x4000;
