use super::{PowerControllerError, Result};
use bq24296m::{
    BatteryLowVoltageThreshold, BatteryRechargeThreshold, BoostCurrentLimit, BoostHotThreshold,
    ChargeTimer, ConfigurationRegisters, InputCurrentLimit, NewFaultRegister,
    PowerOnConfigurationRegister, StatusRegisters, SystemStatusRegister,
    ThermalRegulationThreshold, WatchdogTimer, BQ24296,
};
use embedded_hal::i2c::I2c;
use esp_hal::gpio::*;
use esp_hal::peripherals::GPIO15;
use pcf857x::{OutputPin as ExpanderOutputPin, Pcf8574};
use pcf857x::{P0, P1, P3, P4, P5, P6, P7};

pub struct PowerControllerIO<I2C: I2c> {
    pub charger_i2c: I2C,
    pub pcf8574_i2c: I2C,
    pub boost_converter_enable: GPIO15<'static>,
}

pub struct PowerControllerConfig {
    pub precharge_current: u32,
    pub charging_current: u32,
    pub termination_current: u32,
    pub charging_voltage: u32,
    pub charge_timer: Option<ChargeTimer>,
    pub battary_recharge_threshold: BatteryRechargeThreshold,
    pub battery_low_voltage: BatteryLowVoltageThreshold,

    pub input_current: InputCurrentLimit,
    pub input_voltage: u32,
    pub sys_min_voltage: u32,

    pub boost_voltage: u32,
    pub boost_current_limit: BoostCurrentLimit,
    pub boost_hot_threshold: BoostHotThreshold,
    pub boost_cold_treshold_m20: bool,

    pub i2c_watchdog_timer: WatchdogTimer,
    pub thermal_regulation_treshold: ThermalRegulationThreshold,
    pub enable_charge_fault_int: bool,
    pub enable_battery_fault_int: bool,
}

pub struct PowerControllerStats {
    pub charger_status: SystemStatusRegister,
    pub charger_faults: NewFaultRegister,
}

impl Default for PowerControllerConfig {
    fn default() -> Self {
        Self {
            precharge_current: 512,
            charging_current: 1024,
            termination_current: 100,
            charging_voltage: 4100,
            charge_timer: Some(ChargeTimer::Hours8),
            battary_recharge_threshold: BatteryRechargeThreshold::mV_100,
            battery_low_voltage: BatteryLowVoltageThreshold::mV_3000,

            input_current: InputCurrentLimit::mA_1000,
            input_voltage: 4360,
            sys_min_voltage: 3700,

            boost_voltage: 4998,
            boost_current_limit: BoostCurrentLimit::mA_1000,
            boost_hot_threshold: BoostHotThreshold::Celsius65,
            boost_cold_treshold_m20: true,

            i2c_watchdog_timer: WatchdogTimer::Seconds160,
            thermal_regulation_treshold: ThermalRegulationThreshold::Celsius80,
            enable_charge_fault_int: true,
            enable_battery_fault_int: true,
        }
    }
}

pub enum PowerControllerMode {
    Passive,
    Charging,
    Otg,
}

pub struct PowerController<I2C: I2c> {
    config: PowerControllerConfig,
    mode: PowerControllerMode,
    charger: BQ24296<I2C>,
    expander: Pcf8574<I2C>,
    boost_converter_enable: Output<'static>,
}

struct ExpanderIO<'a, I2C: I2c> {
    chr_en: P0<'a, Pcf8574<I2C>, I2C::Error>,
    chr_otg: P1<'a, Pcf8574<I2C>, I2C::Error>,
    chr_psel: P3<'a, Pcf8574<I2C>, I2C::Error>,
    vbus_flg: P4<'a, Pcf8574<I2C>, I2C::Error>,
    vbus_enable: P5<'a, Pcf8574<I2C>, I2C::Error>,
    vbus_present: P6<'a, Pcf8574<I2C>, I2C::Error>,
    dc_jack_present: P7<'a, Pcf8574<I2C>, I2C::Error>,
}

impl<I2C: I2c> PowerController<I2C> {
    pub fn new(config: PowerControllerConfig, io: PowerControllerIO<I2C>) -> Result<Self, I2C> {
        let charger = BQ24296::new(io.charger_i2c);
        let address = pcf857x::SlaveAddr::Alternative(true, false, true);
        let expander = Pcf8574::new(io.pcf8574_i2c, address);
        let boost_pin = Output::new(
            io.boost_converter_enable,
            Level::Low,
            OutputConfig::default(),
        );

        let mut device = Self {
            config,
            mode: PowerControllerMode::Passive,
            charger,
            expander,
            boost_converter_enable: boost_pin,
        };

        device.setup_expander()?;
        device.write_charger_config()?;

        Ok(device)
    }

    fn setup_expander(&mut self) -> Result<(), I2C> {
        let mut pins = self.expander_pins();

        pins.chr_otg
            .set_high()
            .map_err(PowerControllerError::I2CExpanderError)
    }

    fn write_charger_config(&mut self) -> Result<(), I2C> {
        self.charger
            .transact(|regs: &mut ConfigurationRegisters| {
                regs.ISCR.set_hiz_enabled(false);
                regs.ISCR
                    .set_input_voltage_dpm_mV(self.config.input_voltage);
                regs.ISCR.set_input_current_limit(self.config.input_current);

                regs.POCR.reset_i2c_watchdog();
                regs.POCR
                    .set_system_min_bus_voltage_mV(self.config.sys_min_voltage);
                regs.POCR
                    .set_boost_current_limit(self.config.boost_current_limit);

                regs.CCCR
                    .set_charge_current_limit_mA(self.config.charging_current);
                if self.config.boost_cold_treshold_m20 {
                    regs.CCCR.set_boost_converter_low_temp_to_m20();
                } else {
                    regs.CCCR.set_boost_converter_low_temp_to_m10();
                }

                regs.PCTCCR
                    .set_precharge_current_mA(self.config.precharge_current);
                regs.PCTCCR
                    .set_termination_current_mA(self.config.termination_current);

                regs.CVCR
                    .set_charge_voltage_limit_mV(self.config.charging_voltage);
                regs.CVCR
                    .set_battery_low_voltage_threshold(self.config.battery_low_voltage);
                regs.CVCR
                    .set_battery_recharge_threshold(self.config.battary_recharge_threshold);

                regs.CTTCR.enable_termination();
                regs.CTTCR
                    .set_watchdog_timer(self.config.i2c_watchdog_timer);
                match self.config.charge_timer {
                    Some(dt) => {
                        regs.CTTCR.set_charge_timer(dt);
                        regs.CTTCR.enable_safety_timer();
                    }
                    None => regs.CTTCR.disable_safety_timer(),
                }

                regs.BVTRR.set_boost_voltage_mV(self.config.boost_voltage);
                regs.BVTRR
                    .set_boost_hot_temperature_threshold(self.config.boost_hot_threshold);
                regs.BVTRR
                    .set_thermal_regulation_threshold(self.config.thermal_regulation_treshold);

                regs.MOCR.enable_dpdm_detection();
                regs.MOCR.disable_timer_2x();
                regs.MOCR.enable_batfet();
                let int_mask = match (
                    self.config.enable_charge_fault_int,
                    self.config.enable_battery_fault_int,
                ) {
                    (false, false) => 0u8,
                    (false, true) => 1u8,
                    (true, false) => 2u8,
                    (true, true) => 3u8,
                };
                regs.MOCR.set_interrupt_mask(int_mask);
            })
            .map_err(PowerControllerError::I2cBusError)
    }

    pub fn reconfigure(&mut self, f: impl FnOnce(&mut PowerControllerConfig)) -> Result<(), I2C> {
        f(&mut self.config);
        self.write_charger_config()
    }

    pub fn switch_mode(&mut self, mode: PowerControllerMode) -> Result<(), I2C> {
        let mut pins = self.expander_pins();

        match mode {
            PowerControllerMode::Passive => {
                pins.chr_en
                    .set_high()
                    .map_err(PowerControllerError::I2CExpanderError)?;
                pins.vbus_enable
                    .set_low()
                    .map_err(PowerControllerError::I2CExpanderError)?;
                self.charger
                    .transact(|r: &mut PowerOnConfigurationRegister| {
                        r.disable_charging();
                        r.disable_otg();
                    })
                    .map_err(PowerControllerError::I2cBusError)?;
            }
            PowerControllerMode::Charging => {
                pins.chr_en
                    .set_low()
                    .map_err(PowerControllerError::I2CExpanderError)?;
                pins.vbus_enable
                    .set_low()
                    .map_err(PowerControllerError::I2CExpanderError)?;
                self.charger
                    .transact(|r: &mut PowerOnConfigurationRegister| {
                        r.enable_charging();
                        r.disable_otg();
                    })
                    .map_err(PowerControllerError::I2cBusError)?;
            }
            PowerControllerMode::Otg => {
                pins.chr_en
                    .set_high()
                    .map_err(PowerControllerError::I2CExpanderError)?;
                pins.vbus_enable
                    .set_high()
                    .map_err(PowerControllerError::I2CExpanderError)?;
                self.charger
                    .transact(|r: &mut PowerOnConfigurationRegister| {
                        r.disable_charging();
                        r.enable_otg();
                    })
                    .map_err(PowerControllerError::I2cBusError)?;
            }
        }

        self.mode = mode;

        Ok(())
    }

    pub fn read_stats(&mut self) -> Result<PowerControllerStats, I2C> {
        let stats: StatusRegisters = self
            .charger
            .read()
            .map_err(PowerControllerError::I2cBusError)?;

        Ok(PowerControllerStats {
            charger_status: stats.SSR,
            charger_faults: stats.NFR,
        })
    }

    pub fn reset_watchdog(&mut self) -> Result<(), I2C> {
        self.charger
            .transact(|r: &mut PowerOnConfigurationRegister| {
                r.reset_i2c_watchdog();
            })
            .map_err(PowerControllerError::I2cBusError)?;

        Ok(())
    }

    pub fn get_mode(&self) -> &PowerControllerMode {
        &self.mode
    }

    pub fn enable_boost_converter(&mut self) {
        self.boost_converter_enable.set_high();
    }

    pub fn disable_boost_converter(&mut self) {
        self.boost_converter_enable.set_low();
    }

    pub fn is_boost_converter_enabled(&self) -> bool {
        self.boost_converter_enable.is_set_high()
    }

    fn expander_pins(&mut self) -> ExpanderIO<'_, I2C> {
        let pins = self.expander.split();

        ExpanderIO {
            chr_en: pins.p0,
            chr_otg: pins.p1,
            chr_psel: pins.p3,
            vbus_flg: pins.p4,
            vbus_enable: pins.p5,
            vbus_present: pins.p6,
            dc_jack_present: pins.p7,
        }
    }
}
