# main_oop.py
import usb_hid
from hid.keyboard import Keyboard
from hid.keycode import Keycode
from hid.consumer_control import ConsumerControl
from hid.consumer_control_code import ConsumerControlCode
from hid.keyboard_layout_us import KeyboardLayoutUS
import board
import microcontroller
import digitalio
import time
import pwmio
import supervisor
import sys
# from typing import Optional, Set, List, Tuple, Dict, Callable, Any


# ============================================================
# 配置类
# ============================================================
class Config:
    """系统配置"""
    DEBUG = True
    PWM_FREQUENCY = 5000
    DEBOUNCE_DELAY = 0.05
    SCAN_INTERVAL = 0.01
    BREATHING_STEP = 255
    BREATHING_INTERVAL = 0.3
    PWM_STEP = 6553
    PWM_MAX = 65535
    PWM_MIN = 0
    INITIAL_PWM = 32767


# ============================================================
# 日志工具类
# ============================================================
class Logger:
    """日志工具"""
    
    @staticmethod
    def debug(*args, **kwargs):
        if Config.DEBUG:
            try:
                print(*args, **kwargs)
            except:
                pass
    
    @staticmethod
    def info(*args, **kwargs):
        try:
            print("[INFO]", *args, **kwargs)
        except:
            pass
    
    @staticmethod
    def error(*args, **kwargs):
        try:
            print("[ERROR]", *args, **kwargs)
        except:
            pass


# ============================================================
# GPIO 引脚管理类
# ============================================================
class PinManager:
    """GPIO 引脚管理器"""
    
    def __init__(self):
        self._pins: Dict[str, digitalio.DigitalInOut] = {}
        self._pwms: Dict[str, pwmio.PWMOut] = {}
    
    def add_digital_output(self, name: str, pin: microcontroller.Pin, 
                           initial_value: bool = False) -> 'PinManager':
        """添加数字输出引脚"""
        digital_pin = digitalio.DigitalInOut(pin)
        digital_pin.direction = digitalio.Direction.OUTPUT
        digital_pin.value = initial_value
        self._pins[name] = digital_pin
        return self
    
    def add_pwm_output(self, name: str, pin: microcontroller.Pin,
                       frequency: int = Config.PWM_FREQUENCY,
                       duty_cycle: int = Config.INITIAL_PWM) -> 'PinManager':
        """添加 PWM 输出引脚"""
        pwm = pwmio.PWMOut(pin, frequency=frequency, duty_cycle=duty_cycle)
        self._pwms[name] = pwm
        return self
    
    def get_pin(self, name: str) -> digitalio.DigitalInOut:
        """获取数字引脚"""
        return self._pins[name]
    
    def get_pwm(self, name: str) -> pwmio.PWMOut:
        """获取 PWM 引脚"""
        return self._pwms[name]
    
    def set_pin(self, name: str, value: bool):
        """设置引脚值"""
        if name in self._pins:
            self._pins[name].value = value
    
    def toggle_pin(self, name: str):
        """切换引脚状态"""
        if name in self._pins:
            self._pins[name].value = not self._pins[name].value
    
    def get_pwm_value(self, name: str) -> int:
        """获取 PWM 占空比"""
        return self._pwms[name].duty_cycle if name in self._pwms else 0
    
    def set_pwm_value(self, name: str, value: int):
        """设置 PWM 占空比"""
        if name in self._pwms:
            value = max(Config.PWM_MIN, min(Config.PWM_MAX, value))
            self._pwms[name].duty_cycle = value


# ============================================================
# HID 设备管理类
# ============================================================
class HIDManager:
    """USB HID 设备管理器"""
    
    def __init__(self):
        self._keyboard: Optional[Keyboard] = None
        self._layout: Optional[KeyboardLayoutUS] = None
        self._consumer_control: Optional[ConsumerControl] = None
        self._initialized = False
    
    @property
    def is_initialized(self) -> bool:
        return self._initialized
    
    @property
    def keyboard(self) -> Optional[Keyboard]:
        return self._keyboard
    
    @property
    def layout(self) -> Optional[KeyboardLayoutUS]:
        return self._layout
    
    @property
    def consumer_control(self) -> Optional[ConsumerControl]:
        return self._consumer_control
    
    def init(self) -> bool:
        """初始化 HID 设备"""
        try:
            if not usb_hid.devices:
                Logger.debug("[HID] ⚠️ 无 HID 设备")
                return False
            
            self._keyboard = Keyboard(usb_hid.devices)
            self._layout = KeyboardLayoutUS(self._keyboard)
            self._consumer_control = ConsumerControl(usb_hid.devices)
            self._initialized = True
            Logger.debug("[HID] ✅ HID 已就绪")
            return True
        except Exception as e:
            Logger.error(f"[HID] ❌ 初始化失败: {e}")
            self._reset()
            return False
    
    def _reset(self):
        """重置 HID 设备"""
        self._keyboard = None
        self._layout = None
        self._consumer_control = None
        self._initialized = False
    
    def ensure_ready(self):
        """确保 HID 可用（支持热插拔）"""
        usb_connected = supervisor.runtime.usb_connected
        
        if usb_connected and not self._initialized:
            Logger.debug("[HID] 🔌 USB 插入，初始化 HID...")
            self.init()
        elif not usb_connected and self._initialized:
            Logger.debug("[HID] ⚠️ USB 拔出，释放 HID")
            self._reset()
    
    def press_key(self, *keys):
        """按下按键"""
        if self._keyboard:
            self._keyboard.press(*keys)
    
    def release_key(self, *keys):
        """释放按键"""
        if self._keyboard:
            self._keyboard.release(*keys)
    
    def release_all(self):
        """释放所有按键"""
        if self._keyboard:
            self._keyboard.release_all()
    
    def press_consumer(self, code):
        """按下消费控制键"""
        if self._consumer_control:
            self._consumer_control.press(code)
    
    def release_consumer(self):
        """释放消费控制键"""
        if self._consumer_control:
            self._consumer_control.release()


# ============================================================
# 键盘矩阵扫描类
# ============================================================
class MatrixScanner:
    """矩阵键盘扫描器"""
    
    def __init__(self, pin_manager: PinManager, row_pins: List[microcontroller.Pin],
                 col_pins: List[microcontroller.Pin], key_map: List[List],
                 fn_map: List[List], fn_key_code: int):
        self._pin_manager = pin_manager
        self._row_pins = row_pins
        self._col_pins = col_pins
        self._key_map = key_map
        self._fn_map = fn_map
        self._fn_key_code = fn_key_code
        
        self._row_gpio = self._setup_rows()
        self._col_gpio = self._setup_cols()
    
    def _setup_rows(self):
        """设置行引脚"""
        rows = []
        for pin in self._row_pins:
            row = digitalio.DigitalInOut(pin)
            row.direction = digitalio.Direction.INPUT
            row.pull = digitalio.Pull.UP
            rows.append(row)
        return rows
    
    def _setup_cols(self):
        """设置列引脚"""
        cols = []
        for pin in self._col_pins:
            col = digitalio.DigitalInOut(pin)
            col.direction = digitalio.Direction.OUTPUT
            col.value = False
            cols.append(col)
        return cols
    
    def scan(self) -> Set[int]:
        """扫描键盘，返回按下的键码集合"""
        keys_pressed = []
        fn_active = False
        
        for col_index, col_pin in enumerate(self._col_gpio):
            col_pin.direction = digitalio.Direction.OUTPUT
            col_pin.value = True
            
            for row_index, row_pin in enumerate(self._row_gpio):
                row_pin.direction = digitalio.Direction.INPUT
                row_pin.pull = digitalio.Pull.DOWN
                
                if row_pin.value:
                    time.sleep(Config.DEBOUNCE_DELAY)
                    if row_pin.value:
                        key = self._key_map[row_index][col_index]
                        if key == self._fn_key_code:
                            fn_active = True
                        keys_pressed.append((row_index, col_index))
            
            col_pin.value = False
        
        active_map = self._fn_map if fn_active else self._key_map
        
        translated_keys = set()
        for row_index, col_index in keys_pressed:
            key = active_map[row_index][col_index]
            if key is not None:
                translated_keys.add(key)
        
        return translated_keys
    
    def scan_without_debounce(self) -> Set[int]:
        """扫描键盘（不带消抖，用于快速检测）"""
        keys_pressed = []
        fn_active = False
        
        for col_index, col_pin in enumerate(self._col_gpio):
            col_pin.direction = digitalio.Direction.OUTPUT
            col_pin.value = True
            
            for row_index, row_pin in enumerate(self._row_gpio):
                row_pin.direction = digitalio.Direction.INPUT
                row_pin.pull = digitalio.Pull.DOWN
                
                if row_pin.value:
                    key = self._key_map[row_index][col_index]
                    if key == self._fn_key_code:
                        fn_active = True
                    keys_pressed.append((row_index, col_index))
            
            col_pin.value = False
        
        active_map = self._fn_map if fn_active else self._key_map
        
        translated_keys = set()
        for row_index, col_index in keys_pressed:
            key = active_map[row_index][col_index]
            if key is not None:
                translated_keys.add(key)
        
        return translated_keys


# ============================================================
# 键盘处理器类
# ============================================================
class KeyboardProcessor:
    """键盘事件处理器"""
    
    def __init__(self, hid_manager: HIDManager, scanner: MatrixScanner,
                 special_functions: Dict[int, Callable]):
        self._hid = hid_manager
        self._scanner = scanner
        self._special_functions = special_functions
        self._previous_keys: Set[int] = set()
        self._breathing_active = False
    
    @property
    def breathing_active(self) -> bool:
        return self._breathing_active
    
    @breathing_active.setter
    def breathing_active(self, value: bool):
        self._breathing_active = value
    
    def process(self):
        """处理键盘输入"""
        # 确保 HID 可用
        self._hid.ensure_ready()
        
        # 扫描键盘
        current_keys = self._scanner.scan()
        
        # 如果呼吸灯激活，不处理任何按键（让呼吸灯控制器自己处理）
        if self._breathing_active:
            self._previous_keys = current_keys
            return
        
        # 处理释放的按键
        for key in self._previous_keys - current_keys:
            if key is not None and key >= 0 and self._hid.is_initialized:
                self._hid.release_key(key)
        
        # 处理按下的按键
        for key in current_keys - self._previous_keys:
            if key is not None:
                if key in self._special_functions:
                    self._special_functions[key]()
                elif key >= 0 and self._hid.is_initialized:
                    self._hid.press_key(key)
        
        self._previous_keys = current_keys


# ============================================================
# 呼吸灯控制器（修复版本）
# ============================================================
class BreathingLightController:
    """呼吸灯控制器"""
    
    def __init__(self, pin_manager: PinManager, gp22_name: str, gp21_name: str,
                 hid_manager: HIDManager, scanner: MatrixScanner,
                 processor: KeyboardProcessor):
        self._pin_manager = pin_manager
        self._gp22_name = gp22_name
        self._gp21_name = gp21_name
        self._hid = hid_manager
        self._scanner = scanner
        self._processor = processor
        self._running = False
    
    def start(self):
        """启动呼吸灯效果"""
        if self._running:
            return
        
        self._running = True
        self._processor.breathing_active = True  # 通知处理器进入呼吸灯模式
        
        # 保存原始状态
        gp22_original = self._pin_manager.get_pin(self._gp22_name).value
        
        # 【关键修复】先反转 gp21 息屏
        self._pin_manager.toggle_pin(self._gp21_name)
        Logger.debug(f"[Breathing] 息屏: gp21 = {self._pin_manager.get_pin(self._gp21_name).value}")
        
        try:
            # 闪烁提示（10次）
            for i in range(10):
                if not self._running:
                    break
                self._pin_manager.toggle_pin(self._gp22_name)
                time.sleep(Config.BREATHING_INTERVAL)
            
            # 呼吸灯循环
            counter = 0
            while self._running:
                counter += 1
                if counter >= Config.BREATHING_STEP:
                    self._pin_manager.toggle_pin(self._gp22_name)
                    counter = 0
                
                # 检测按键退出
                current_keys = self._scanner.scan_without_debounce()
                if current_keys:
                    Logger.debug("[Breathing] 检测到按键，退出呼吸灯")
                    self._running = False
                    break
                
                time.sleep(0.01)
        
        finally:
            # 退出呼吸灯模式
            self._processor.breathing_active = False
            
            # 恢复 gp22 到原始状态
            self._pin_manager.set_pin(self._gp22_name, gp22_original)
            
            # 【关键修复】再次反转 gp21 亮屏
            self._pin_manager.toggle_pin(self._gp21_name)
            Logger.debug(f"[Breathing] 亮屏: gp21 = {self._pin_manager.get_pin(self._gp21_name).value}")
            
            # 释放所有 HID 按键
            self._hid.release_all()
            
            # 清空扫描缓存
            time.sleep(0.05)
            self._scanner.scan()
            
            Logger.debug("[Breathing] 呼吸灯已退出")
    
    def stop(self):
        """停止呼吸灯"""
        self._running = False


# ============================================================
# PWM 控制器（修复反转问题）
# ============================================================
class PWMController:
    """PWM 控制器（支持取反电路）"""
    
    def __init__(self, pin_manager: PinManager, pwm_name: str, inverted: bool = False):
        self._pin_manager = pin_manager
        self._pwm_name = pwm_name
        self._step = Config.PWM_STEP
        self._inverted = inverted  # 是否取反
    
    def increase(self):
        """增加占空比"""
        current = self._pin_manager.get_pwm_value(self._pwm_name)
        if self._inverted:
            # 取反电路：增加实际亮度 = 减小 PWM 值
            new_value = max(Config.PWM_MIN, current - self._step)
        else:
            new_value = min(Config.PWM_MAX, current + self._step)
        self._pin_manager.set_pwm_value(self._pwm_name, new_value)
        Logger.debug(f"{self._pwm_name}: {new_value}")
    
    def decrease(self):
        """减少占空比"""
        current = self._pin_manager.get_pwm_value(self._pwm_name)
        if self._inverted:
            # 取反电路：减少实际亮度 = 增大 PWM 值
            new_value = min(Config.PWM_MAX, current + self._step)
        else:
            new_value = max(Config.PWM_MIN, current - self._step)
        self._pin_manager.set_pwm_value(self._pwm_name, new_value)
        Logger.debug(f"{self._pwm_name}: {new_value}")


# ============================================================
# 自定义按键定义
# ============================================================
class CustomKeycodes:
    """自定义按键码"""
    FN_KEY = -100
    FN_MUTE = -101
    FN_VOLUME_DOWN = -102
    FN_VOLUME_UP = -103
    FN_LOCK_SCREEN = -104
    FN_BL_CONTROL_SCREEN = -105
    FN_BL_PWM_DOWN = -106
    FN_BL_PWM_UP = -107
    SHIFT_GRAVE_ACCENT = -108
    SHIFT_BACKSLASH = -109
    SHIFT_LEFT_BRACKET = -110
    SHIFT_RIGHT_BRACKET = -111


# ============================================================
# 主控制器
# ============================================================
class KeyboardController:
    """键盘主控制器"""
    
    def __init__(self):
        # 初始化引脚管理器
        self.pin_manager = PinManager()
        self._setup_pins()
        
        # 初始化 HID
        self.hid = HIDManager()
        self.hid.init()
        
        # 初始化键盘矩阵
        self.scanner = MatrixScanner(
            self.pin_manager,
            row_pins=[board.GP16, board.GP10, board.GP11, board.GP12, 
                      board.GP13, board.GP14, board.GP15],
            col_pins=[board.GP0, board.GP1, board.GP2, board.GP3,
                      board.GP4, board.GP5, board.GP6, board.GP7,
                      board.GP8, board.GP9],
            key_map=self._create_key_map(),
            fn_map=self._create_fn_map(),
            fn_key_code=CustomKeycodes.FN_KEY
        )
        
        # 初始化键盘处理器（先创建，用于依赖注入）
        self.processor = KeyboardProcessor(
            self.hid, self.scanner, {}
        )
        
        # 初始化功能控制器
        self._setup_controllers()
        
        # 设置特殊功能映射（在控制器初始化后）
        self._setup_special_functions()
        
        # 更新处理器的特殊功能映射
        self.processor._special_functions = self._special_functions
    
    def _setup_pins(self):
        """设置引脚"""
        self.pin_manager.add_digital_output("gp22", board.GP22, False)
        self.pin_manager.add_digital_output("gp19", board.GP19, False)
        self.pin_manager.add_digital_output("gp21", board.GP21, False)
        self.pin_manager.add_pwm_output("bl_pwm", board.GP20, duty_cycle=5000)
        self.pin_manager.add_pwm_output("ad_pwm", board.GP18, duty_cycle=32700)
    
    def _setup_controllers(self):
        """设置功能控制器"""
        self.breathing_light = BreathingLightController(
            self.pin_manager, "gp22", "gp21", self.hid, self.scanner, self.processor
        )
        # 背光 PWM 设置了 inverted=True（因为有取反电路）
        self.bl_pwm = PWMController(self.pin_manager, "bl_pwm", inverted=True)
        self.ad_pwm = PWMController(self.pin_manager, "ad_pwm", inverted=False)
    
    def _setup_special_functions(self):
        """设置特殊功能映射"""
        self._special_functions = {
            CustomKeycodes.FN_MUTE: lambda: self.pin_manager.toggle_pin("gp19"),
            CustomKeycodes.FN_VOLUME_DOWN: self.ad_pwm.decrease,
            CustomKeycodes.FN_VOLUME_UP: self.ad_pwm.increase,
            CustomKeycodes.FN_LOCK_SCREEN: self._lock_screen,
            CustomKeycodes.FN_BL_CONTROL_SCREEN: self._toggle_breathing_light,
            CustomKeycodes.FN_BL_PWM_DOWN: self.bl_pwm.decrease,
            CustomKeycodes.FN_BL_PWM_UP: self.bl_pwm.increase,
            CustomKeycodes.SHIFT_GRAVE_ACCENT: self._shift_grave_accent,
            CustomKeycodes.SHIFT_BACKSLASH: self._shift_backslash,
            CustomKeycodes.SHIFT_LEFT_BRACKET: self._shift_left_bracket,
            CustomKeycodes.SHIFT_RIGHT_BRACKET: self._shift_right_bracket,
            Keycode.CAPS_LOCK: self._toggle_gp22,
            ConsumerControlCode.SCAN_PREVIOUS_TRACK: self._scan_previous_track,
            ConsumerControlCode.PLAY_PAUSE: self._play_pause,
            ConsumerControlCode.SCAN_NEXT_TRACK: self._scan_next_track,
        }
    
    def _create_key_map(self) -> List[List]:
        """创建普通键位映射"""
        return [
            # pt35-desktop: the six face buttons send F13-F18, not the letters
            # l r x y b a. A button press is then never a character, so the
            # shell can bind it everywhere without stealing the keyboard.
            [Keycode.UP_ARROW, Keycode.LEFT_ARROW, Keycode.DOWN_ARROW,
             Keycode.RIGHT_ARROW, Keycode.F13, Keycode.F14, Keycode.F15,
             Keycode.F16, Keycode.F17, Keycode.F18],
            [Keycode.ONE, Keycode.TWO, Keycode.THREE, Keycode.FOUR, 
             Keycode.FIVE, Keycode.SIX, Keycode.SEVEN, Keycode.EIGHT, 
             Keycode.NINE, Keycode.ZERO],
            [Keycode.Q, Keycode.W, Keycode.E, Keycode.R, Keycode.T, 
             Keycode.Y, Keycode.U, Keycode.I, Keycode.O, Keycode.P],
            [Keycode.A, Keycode.S, Keycode.D, Keycode.F, Keycode.G, 
             Keycode.H, Keycode.J, Keycode.K, Keycode.L, Keycode.BACKSPACE],
            [Keycode.Z, Keycode.X, Keycode.C, Keycode.V, Keycode.B, 
             Keycode.N, Keycode.M, Keycode.FORWARD_SLASH, Keycode.ENTER, None],
            [Keycode.TAB, Keycode.CAPS_LOCK, Keycode.MINUS, Keycode.EQUALS, 
             Keycode.SEMICOLON, Keycode.QUOTE, Keycode.COMMA, 
             Keycode.PERIOD, Keycode.SHIFT, None],
            # pt35-desktop: Select and Start send F21 and F22, keys of their
            # own, so Fn+Select and Fn+Start can be the real Print Screen and
            # Pause the keycaps show.
            [CustomKeycodes.FN_KEY, Keycode.CONTROL, Keycode.LEFT_ALT, 
             Keycode.F21, Keycode.SPACE, Keycode.F22, 
             Keycode.RIGHT_ALT, Keycode.WINDOWS, CustomKeycodes.FN_KEY, None]
        ]
    
    def _create_fn_map(self) -> List[List]:
        """创建 FN 键位映射"""
        return [
            # pt35-desktop: the six face buttons send F13-F18, not the letters
            # l r x y b a. A button press is then never a character, so the
            # shell can bind it everywhere without stealing the keyboard.
            [Keycode.UP_ARROW, Keycode.LEFT_ARROW, Keycode.DOWN_ARROW,
             Keycode.RIGHT_ARROW, Keycode.F13, Keycode.F14, Keycode.F15,
             Keycode.F16, Keycode.F17, Keycode.F18],
            [Keycode.F1, Keycode.F2, Keycode.F3, Keycode.F4,
             Keycode.F5, Keycode.F6, Keycode.F7, Keycode.F8,
             Keycode.F9, Keycode.F10],
            [Keycode.ESCAPE, CustomKeycodes.FN_MUTE, CustomKeycodes.FN_VOLUME_DOWN,
             CustomKeycodes.FN_VOLUME_UP, ConsumerControlCode.SCAN_PREVIOUS_TRACK,
             ConsumerControlCode.PLAY_PAUSE, ConsumerControlCode.SCAN_NEXT_TRACK,
             CustomKeycodes.FN_LOCK_SCREEN, Keycode.F11, Keycode.F12],
            [Keycode.GRAVE_ACCENT, CustomKeycodes.SHIFT_GRAVE_ACCENT,
             Keycode.BACKSLASH, CustomKeycodes.SHIFT_BACKSLASH,
             CustomKeycodes.SHIFT_LEFT_BRACKET, CustomKeycodes.SHIFT_RIGHT_BRACKET,
             Keycode.LEFT_BRACKET, Keycode.RIGHT_BRACKET, Keycode.L,
             Keycode.DELETE],
            [Keycode.INSERT, Keycode.HOME, CustomKeycodes.FN_BL_CONTROL_SCREEN,
             Keycode.END, Keycode.PAGE_UP, Keycode.PAGE_DOWN,
             Keycode.SCROLL_LOCK, Keycode.FORWARD_SLASH, Keycode.ENTER, None],
            [Keycode.TAB, Keycode.CAPS_LOCK, CustomKeycodes.FN_BL_PWM_DOWN,
             CustomKeycodes.FN_BL_PWM_UP, Keycode.SEMICOLON, Keycode.QUOTE,
             Keycode.COMMA, Keycode.PERIOD, Keycode.SHIFT, None],
            [CustomKeycodes.FN_KEY, Keycode.CONTROL, Keycode.LEFT_ALT,
             Keycode.PRINT_SCREEN, Keycode.SPACE, Keycode.PAUSE,
             Keycode.RIGHT_ALT, Keycode.WINDOWS, CustomKeycodes.FN_KEY, None]
        ]
    
    # ---- 功能方法 ----
    
    def _toggle_gp22(self):
        """切换 GP22 并发送 CAPS_LOCK"""
        self.pin_manager.toggle_pin("gp22")
        Logger.debug(f"GP22: {self.pin_manager.get_pin('gp22').value}")
        if self.hid.is_initialized:
            self.hid.press_key(Keycode.CAPS_LOCK)
            self.hid.release_key(Keycode.CAPS_LOCK)
    
    def _toggle_breathing_light(self):
        """切换呼吸灯（息屏）"""
        Logger.debug("[Controller] Fn+C 触发息屏")
        self.breathing_light.start()
    
    def _lock_screen(self):
        """锁定屏幕"""
        if self.hid.is_initialized:
            self.hid.press_key(Keycode.WINDOWS, Keycode.L)
            self.hid.release_all()
    
    def _shift_grave_accent(self):
        if self.hid.is_initialized:
            self.hid.press_key(Keycode.SHIFT, Keycode.GRAVE_ACCENT)
            self.hid.release_all()
    
    def _shift_backslash(self):
        if self.hid.is_initialized:
            self.hid.press_key(Keycode.SHIFT, Keycode.BACKSLASH)
            self.hid.release_all()
    
    def _shift_left_bracket(self):
        if self.hid.is_initialized:
            self.hid.press_key(Keycode.SHIFT, Keycode.LEFT_BRACKET)
            self.hid.release_all()
    
    def _shift_right_bracket(self):
        if self.hid.is_initialized:
            self.hid.press_key(Keycode.SHIFT, Keycode.RIGHT_BRACKET)
            self.hid.release_all()
    
    def _scan_previous_track(self):
        if self.hid.is_initialized:
            self.hid.press_consumer(ConsumerControlCode.SCAN_PREVIOUS_TRACK)
            self.hid.release_consumer()
    
    def _play_pause(self):
        if self.hid.is_initialized:
            self.hid.press_consumer(ConsumerControlCode.PLAY_PAUSE)
            self.hid.release_consumer()
    
    def _scan_next_track(self):
        if self.hid.is_initialized:
            self.hid.press_consumer(ConsumerControlCode.SCAN_NEXT_TRACK)
            self.hid.release_consumer()
    
    # pt35-desktop: the backlight from Linux. The host writes `B<0-100>` or
    # `B?` and a newline on the USB console; the answer is `PT35 B=<percent>`,
    # a line nothing else here prints. 5% is the floor: a black panel on a
    # device with no other screen is not a brightness.
    BL_FLOOR = 5

    def _backlight_percent(self):
        duty = self.pin_manager.get_pwm_value("bl_pwm")
        # Inverted circuit: duty 0 is full brightness.
        return (Config.PWM_MAX - duty) * 100 // Config.PWM_MAX

    def _host_command(self, line):
        line = line.strip()
        if line == "B?":
            pass
        elif line.startswith("B") and line[1:].isdigit():
            percent = max(self.BL_FLOOR, min(100, int(line[1:])))
            duty = Config.PWM_MAX - percent * Config.PWM_MAX // 100
            self.pin_manager.set_pwm_value("bl_pwm", duty)
        else:
            return
        print("PT35 B=%d" % self._backlight_percent())

    def _poll_host(self):
        # Only what has already arrived: the scan loop must never wait on it.
        while supervisor.runtime.serial_bytes_available:
            ch = sys.stdin.read(1)
            if ch in "\r\n":
                self._host_command(self._host_line)
                self._host_line = ""
            elif len(self._host_line) < 16:
                self._host_line += ch

    def run(self):
        """主循环"""
        Logger.debug("[System] 系统启动完成")
        Logger.debug("[System] 等待键盘输入...")
        self._host_line = ""
        
        while True:
            self.processor.process()
            self._poll_host()
            time.sleep(Config.SCAN_INTERVAL)


# ============================================================
# 程序入口
# ============================================================
if __name__ == "__main__":
    controller = KeyboardController()
    controller.run()