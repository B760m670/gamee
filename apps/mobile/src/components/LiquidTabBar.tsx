import React, {useEffect, useRef, useState} from 'react';
import {View, Animated, PanResponder, StyleSheet} from 'react-native';
import {useSafeAreaInsets} from 'react-native-safe-area-context';
import type {BottomTabBarProps} from '@react-navigation/bottom-tabs';
import {TabBarItem} from './TabBarItem';
import {colors} from '../theme';

// Recreation of the iOS 26 Liquid Glass tab bar for Android:
// a floating translucent capsule with a sliding pill that can be dragged with
// the finger; the screen switches only when the finger is released.
// (Apple's "glassy blob" deformation uses a private API and is not replicable.)

export function LiquidTabBar({state, navigation}: BottomTabBarProps) {
  const insets = useSafeAreaInsets();
  const [barW, setBarW] = useState(0);
  const count = state.routes.length;
  const itemW = barW > 0 ? barW / count : 0;

  const pillX = useRef(new Animated.Value(0)).current;
  const itemWRef = useRef(0);
  const countRef = useRef(count);
  const indexRef = useRef(state.index);
  const draggingRef = useRef(false);
  itemWRef.current = itemW;
  countRef.current = count;
  indexRef.current = state.index;

  // Snap the pill to the active tab when the index changes (and not dragging).
  useEffect(() => {
    if (itemW > 0 && !draggingRef.current) {
      Animated.spring(pillX, {
        toValue: state.index * itemW,
        useNativeDriver: false,
        speed: 16,
        bounciness: 8,
      }).start();
    }
  }, [state.index, itemW, pillX]);

  function go(index: number) {
    const route = state.routes[index];
    const focused = state.index === index;
    const event = navigation.emit({
      type: 'tabPress',
      target: route.key,
      canPreventDefault: true,
    });
    if (!focused && !event.defaultPrevented) {
      navigation.navigate(route.name);
    }
  }

  const pan = useRef(
    PanResponder.create({
      // Taps fall through to the items; only a horizontal drag grabs the pill.
      onStartShouldSetPanResponder: () => false,
      onMoveShouldSetPanResponder: (_e, g) =>
        Math.abs(g.dx) > 6 && Math.abs(g.dx) > Math.abs(g.dy),
      onPanResponderGrant: () => {
        draggingRef.current = true;
      },
      onPanResponderMove: (_e, g) => {
        const w = itemWRef.current;
        if (w <= 0) return;
        const max = (countRef.current - 1) * w;
        let x = indexRef.current * w + g.dx;
        x = Math.max(0, Math.min(x, max));
        pillX.setValue(x);
      },
      onPanResponderRelease: (_e, g) => {
        const w = itemWRef.current;
        draggingRef.current = false;
        if (w <= 0) return;
        const max = (countRef.current - 1) * w;
        let x = indexRef.current * w + g.dx;
        x = Math.max(0, Math.min(x, max));
        let target = Math.round(x / w);
        target = Math.max(0, Math.min(target, countRef.current - 1));
        Animated.spring(pillX, {
          toValue: target * w,
          useNativeDriver: false,
          speed: 16,
          bounciness: 8,
        }).start();
        // Commit the switch only on release.
        go(target);
      },
      onPanResponderTerminate: () => {
        draggingRef.current = false;
      },
    }),
  ).current;

  return (
    <View style={[s.wrap, {paddingBottom: insets.bottom + 8}]}>
      <View
        style={s.bar}
        onLayout={e => setBarW(e.nativeEvent.layout.width)}
        {...pan.panHandlers}>
        {itemW > 0 ? (
          <Animated.View
            pointerEvents="none"
            style={[s.pill, {width: itemW, transform: [{translateX: pillX}]}]}
          />
        ) : null}
        <View style={s.row} pointerEvents="box-none">
          {state.routes.map((route, i) => (
            <TabBarItem
              key={route.key}
              name={route.name}
              focused={state.index === i}
              onPress={() => go(i)}
            />
          ))}
        </View>
      </View>
    </View>
  );
}

const s = StyleSheet.create({
  wrap: {backgroundColor: colors.bg, paddingHorizontal: 14, paddingTop: 8},
  bar: {
    height: 62,
    borderRadius: 31,
    backgroundColor: 'rgba(255,255,255,0.06)',
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: 'rgba(255,255,255,0.14)',
    overflow: 'hidden',
    justifyContent: 'center',
  },
  pill: {
    position: 'absolute',
    top: 6,
    bottom: 6,
    left: 0,
    borderRadius: 25,
    backgroundColor: 'rgba(47,123,255,0.22)',
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: 'rgba(47,123,255,0.45)',
  },
  row: {flexDirection: 'row', alignItems: 'center'},
});
