use core::cell::Cell;
use kernel::common::cells::{OptionalCell, TakeCell};
use kernel::hil::rfcore;
use kernel::ReturnCode;
use osc;
use radio::commands::{prop_commands as prop, DirectCommand, RadioCommand, RfcCondition};
use radio::patch_cpe_prop as cpe;
use radio::patch_mce_genfsk as mce;
use radio::patch_mce_longrange as mce_lr;
use radio::patch_rfe_genfsk as rfe;
use radio::rfc;
use rtc;

// const TEST_PAYLOAD: [u32; 30] = [0; 30];

static mut GFSK_RFPARAMS: [u32; 25] = [
    // override_use_patch_prop_genfsk.xml
    0x00000847, // PHY: Use MCE RAM patch, RFE RAM patch MCE_RFE_OVERRIDE(1,0,0,1,0,0),
    // override_synth_prop_863_930_div5.xml
    0x02400403, // Synth: Use 48 MHz crystal as synth clock, enable extra PLL filtering
    0x00068793, // Synth: Set minimum RTRIM to 6
    0x001C8473, // Synth: Configure extra PLL filtering
    0x00088433, // Synth: Configure extra PLL filtering
    0x000684A3, // Synth: Set Fref to 4 MHz
    0x40014005, // Synth: Configure faster calibration HW32_ARRAY_OVERRIDE(0x4004,1),
    0x180C0618, // Synth: Configure faster calibration
    0xC00401A1, // Synth: Configure faster calibration
    0x00010101, // Synth: Configure faster calibration
    0xC0040141, // Synth: Configure faster calibration
    0x00214AD3, // Synth: Configure faster calibration
    0x02980243, // Synth: Decrease synth programming time-out by 90 us from default (0x0298 RAT ticks = 166 us) Synth: Set loop bandwidth after lock to 20 kHz
    0x0A480583, // Synth: Set loop bandwidth after lock to 20 kHz
    0x7AB80603, // Synth: Set loop bandwidth after lock to 20 kHz
    0x00000623,
    // override_phy_tx_pa_ramp_genfsk.xml
    0x50880002, // Tx: Configure PA ramp time, PACTL2.RC=0x3 (in ADI0, set PACTL2[3]=1) ADI_HALFREG_OVERRIDE(0,16,0x8,0x8),
    0x51110002, // Tx: Configure PA ramp time, PACTL2.RC=0x3 (in ADI0, set PACTL2[4]=1) ADI_HALFREG_OVERRIDE(0,17,0x1,0x1),
    // override_phy_rx_frontend_genfsk.xml
    0x001a609c, // Rx: Set AGC reference level to 0x1A (default: 0x2E) HW_REG_OVERRIDE(0x609C,0x001A),
    0x00018883, // Rx: Set LNA bias current offset to adjust +1 (default: 0)
    0x000288A3, // Rx: Set RSSI offset to adjust reported RSSI by -2 dB (default: 0)
    // override_phy_rx_aaf_bw_0xd.xml
    0x7ddf0002, // Rx: Set anti-aliasing filter bandwidth to 0xD (in ADI0, set IFAMPCTL3[7:4]=0xD) ADI_HALFREG_OVERRIDE(0,61,0xF,0xD),
    0xFFFC08C3, // TX power override DC/DC regulator: In Tx with 14 dBm PA setting, use DCDCCTL5[3:0]=0xF (DITHER_EN=1 and IPEAK=7). In Rx, use DCDCCTL5[3:0]=0xC (DITHER_EN=1 and IPEAK=4).
    0x0cf80002, // Tx: Set PA trim to max to maximize its output power (in ADI0, set PACTL0=0xF8) ADI_REG_OVERRIDE(0,12,0xF8),
    0xFFFFFFFF, // Stop word
];

type MultiModeResult = Result<(), ReturnCode>;

#[allow(unused)]
#[derive(Copy, Clone)]
pub enum CpePatch {
    GenFsk { patch: cpe::Patches },
}

#[allow(unused)]
#[derive(Copy, Clone)]
pub enum RfePatch {
    #[derive(Copy, Clone)]
    GenFsk { patch: rfe::Patches },
}

#[allow(unused)]
#[derive(Copy, Clone)]
pub enum McePatch {
    GenFsk { patch: mce::Patches },
    LongRange { patch: mce_lr::Patches },
}

#[allow(unused)]
#[derive(Copy, Clone)]
pub struct RadioMode {
    mode: rfc::RfcMode,
    cpe_patch: CpePatch,
    rfe_patch: RfePatch,
    mce_patch: McePatch,
}

impl Default for RadioMode {
    fn default() -> RadioMode {
        RadioMode {
            mode: rfc::RfcMode::Unchanged,
            cpe_patch: CpePatch::GenFsk {
                patch: cpe::CPE_PATCH,
            },
            rfe_patch: RfePatch::GenFsk {
                patch: rfe::RFE_PATCH,
            },
            mce_patch: McePatch::GenFsk {
                patch: mce::MCE_PATCH,
            },
        }
    }
}

#[allow(unused)]
#[derive(Copy, Clone)]
pub enum RadioSetupCommand {
    Ble,
    PropGfsk { cmd: prop::CommandRadioDivSetup },
}

#[allow(unused)]
pub struct Radio {
    rfc: &'static rfc::RFCore,
    mode: OptionalCell<RadioMode>,
    setup: OptionalCell<RadioSetupCommand>,
    tx_client: OptionalCell<&'static rfcore::TxClient>,
    rx_client: OptionalCell<&'static rfcore::RxClient>,
    cfg_client: OptionalCell<&'static rfcore::ConfigClient>,
    update_config: Cell<bool>,
    schedule_powerdown: Cell<bool>,
    yeilded: Cell<bool>,
    tx_buf: TakeCell<'static, [u8]>,
    rx_buf: TakeCell<'static, [u8]>,
}

impl Radio {
    pub const fn new(rfc: &'static rfc::RFCore) -> Radio {
        Radio {
            rfc,
            mode: OptionalCell::empty(),
            setup: OptionalCell::empty(),
            tx_client: OptionalCell::empty(),
            rx_client: OptionalCell::empty(),
            cfg_client: OptionalCell::empty(),
            update_config: Cell::new(false),
            schedule_powerdown: Cell::new(false),
            yeilded: Cell::new(false),
            tx_buf: TakeCell::empty(),
            rx_buf: TakeCell::empty(),
        }
    }

    pub fn power_up(&self) -> MultiModeResult {
        // TODO Need so have some mode setting done in initialize callback perhaps to pass into
        // power_up() here, the RadioMode enum is defined above which will set a mode in this
        // multimode context along with applying the patches which are attached. Maybe it would be
        // best for the client to just pass an int for the mode and do it all here? not sure yet.

        // self.mode.set(m);

        self.rfc.set_mode(rfc::RfcMode::BLE);

        osc::OSC.request_switch_to_hf_xosc();

        self.rfc.enable();

        self.rfc.start_rat();

        osc::OSC.switch_to_hf_xosc();

        // Need to match on patches here but for now, just default to genfsk patches
        mce::MCE_PATCH.apply_patch();
        rfe::RFE_PATCH.apply_patch();

        unsafe {
            let reg_overrides: u32 = GFSK_RFPARAMS.as_mut_ptr() as u32;

            let status = self.rfc.setup(reg_overrides, 0x9F3F);
            match status {
                ReturnCode::SUCCESS => Ok(()),
                _ => Err(status),
            }
        }
    }

    pub fn power_down(&self) -> MultiModeResult {
        let status = self.rfc.disable();
        match status {
            ReturnCode::SUCCESS => Ok(()),
            _ => Err(status),
        }
    }

    /*
    unsafe fn move_tx_buffer(&self, buf: &'static mut [u8], len: usize) -> &'static mut [u8] {
        for (i,c) in buf.as_ref()[0..len].iter().enumerate() {

        }
    }
    */
}

impl rfc::RFCoreClient for Radio {
    fn command_done(&self) {
        unsafe { rtc::RTC.sync() };

        if self.schedule_powerdown.get() {
            // TODO Need to handle powerdown failure here or we will not be able to enter low power
            // modes
            self.power_down().ok();
            osc::OSC.switch_to_hf_rcosc();

            self.schedule_powerdown.set(false);
            // do sleep mode here later
        }

        self.cfg_client
            .map(|client| client.config_event(ReturnCode::SUCCESS));
    }

    fn tx_done(&self) {
        unsafe { rtc::RTC.sync() };

        if self.schedule_powerdown.get() {
            // TODO Need to handle powerdown failure here or we will not be able to enter low power
            // modes
            self.power_down().ok();
            osc::OSC.switch_to_hf_rcosc();

            self.schedule_powerdown.set(false);
            // do sleep mode here later
        }
        self.tx_buf.take().map_or(ReturnCode::ERESERVE, |tx_buf| {
            self.tx_client
                .map(move |client| client.transmit_event(tx_buf, ReturnCode::SUCCESS));
            ReturnCode::SUCCESS
        });
    }

    fn rx_ok(&self) {
        unsafe { rtc::RTC.sync() };

        self.rx_buf.take().map_or(ReturnCode::ERESERVE, |rx_buf| {
            let frame_len = rx_buf.len();
            let crc_valid = true;
            self.rx_client.map(move |client| {
                client.receive_event(rx_buf, frame_len, crc_valid, ReturnCode::SUCCESS)
            });
            ReturnCode::SUCCESS
        });
    }
}

impl rfcore::Radio for Radio {}

impl rfcore::RadioDriver for Radio {
    fn set_transmit_client(&self, tx_client: &'static rfcore::TxClient) {
        self.tx_client.set(tx_client);
    }

    fn set_receive_client(&self, rx_client: &'static rfcore::RxClient, _rx_buf: &'static mut [u8]) {
        self.rx_client.set(rx_client);
    }

    fn set_receive_buffer(&self, _rx_buf: &'static mut [u8]) {
        // maybe make a rx buf only when needed?
    }

    fn set_config_client(&self, config_client: &'static rfcore::ConfigClient) {
        self.cfg_client.set(config_client);
    }

    fn transmit(
        &self,
        tx_buf: &'static mut [u8],
        _frame_len: usize,
    ) -> (ReturnCode, Option<&'static mut [u8]>) {
        let res = self.tx_buf.replace(tx_buf).map_or_else(
            || {
                // tx_buf is not empty. We do not want to replace the buffer here because it could be
                // in use by the radio so we should schedule some callback for tx_busy
                (ReturnCode::EBUSY, None)
            },
            |tbuf| {
                let p_packet = tbuf.as_mut_ptr() as u32;

                let cmd_tx = prop::CommandTx {
                    command_no: 0x3801,
                    status: 0,
                    p_nextop: 0,
                    start_time: 0,
                    start_trigger: 0,
                    condition: {
                        let mut cond = RfcCondition(0);
                        cond.set_rule(0x01);
                        cond
                    },
                    packet_conf: {
                        let mut packet = prop::RfcPacketConf(0);
                        packet.set_fs_off(false);
                        packet.set_use_crc(true);
                        packet.set_var_len(true);
                        packet
                    },
                    packet_len: 0x14,
                    sync_word: 0x930B51DE,
                    packet_pointer: p_packet,
                };

                let cmd = RadioCommand::pack(cmd_tx);

                self.rfc
                    .send_sync(&cmd)
                    .and_then(|_| self.rfc.wait(&cmd))
                    .ok();

                (ReturnCode::SUCCESS, Some(tbuf))
            },
        );
        res
    }
}

impl rfcore::RadioConfig for Radio {
    fn initialize(&self) -> ReturnCode {
        match self.power_up() {
            Ok(()) => ReturnCode::SUCCESS,
            Err(e) => e,
        }
    }

    fn reset(&self) -> ReturnCode {
        let status = self.power_down().and_then(|_| self.power_up());
        match status {
            Ok(()) => ReturnCode::SUCCESS,
            Err(e) => e,
        }
    }

    fn stop(&self) -> ReturnCode {
        let cmd_stop = DirectCommand::new(0x0402, 0);
        let stopped = self.rfc.send_direct(&cmd_stop).is_ok();
        if stopped {
            ReturnCode::SUCCESS
        } else {
            ReturnCode::FAIL
        }
    }

    fn is_on(&self) -> bool {
        true
    }

    fn busy(&self) -> bool {
        // Might be an obsolete command here in favor of get_command_status and some logic on the
        // user size to determine if the radio is busy. Not sure what is best to have here but
        // arguing best might be bikeshedding
        let status = self.rfc.status.get();
        match status {
            0x0001 => true,
            0x0002 => true,
            _ => false,
        }
    }

    fn config_commit(&self) {
        // TODO confirm set new config here
    }

    fn get_tx_power(&self) -> u32 {
        // TODO get tx power radio command
        0x00000000
    }

    fn get_radio_status(&self) -> u32 {
        // TODO get power status of radio
        0x00000000
    }

    fn get_command_status(&self) -> (ReturnCode, Option<u32>) {
        // TODO get command status specifics
        let status = self.rfc.status.get();
        match status & 0x0F00 {
            0 => (ReturnCode::SUCCESS, Some(status)),
            4 => (ReturnCode::SUCCESS, Some(status)),
            8 => (ReturnCode::FAIL, Some(status)),
            _ => (ReturnCode::EINVAL, Some(status)),
        }
    }

    fn set_tx_power(&self, power: u16) -> ReturnCode {
        // Send direct command for TX power change
        let command = DirectCommand::new(0x0010, power);
        if self.rfc.send_direct(&command).is_ok() {
            return ReturnCode::SUCCESS;
        } else {
            return ReturnCode::FAIL;
        }
    }

    fn send_stop_command(&self) -> ReturnCode {
        // Send "Gracefull" stop radio operation direct command
        let command = DirectCommand::new(0x0402, 0);
        if self.rfc.send_direct(&command).is_ok() {
            return ReturnCode::SUCCESS;
        } else {
            return ReturnCode::FAIL;
        }
    }

    fn send_kill_command(&self) -> ReturnCode {
        // Send immidiate command kill all radio operation commands
        let command = DirectCommand::new(0x0401, 0);
        if self.rfc.send_direct(&command).is_ok() {
            return ReturnCode::SUCCESS;
        } else {
            return ReturnCode::FAIL;
        }
    }
}
