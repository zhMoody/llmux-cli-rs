import{r as g,g as j,R as V}from"./vendor-react-69feda35.js";const m=e=>{let t;const n=new Set,o=(s,f)=>{const a=typeof s=="function"?s(t):s;if(!Object.is(a,t)){const i=t;t=f??(typeof a!="object"||a===null)?a:Object.assign({},t,a),n.forEach(c=>c(t,i))}},r=()=>t,E={setState:o,getState:r,getInitialState:()=>v,subscribe:s=>(n.add(s),()=>n.delete(s)),destroy:()=>{n.clear()}},v=t=e(o,r,E);return E},$=e=>e?m(e):m;var h={exports:{}},R={},w={exports:{}},O={};/**
 * @license React
 * use-sync-external-store-shim.production.js
 *
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */var d=g;function I(e,t){return e===t&&(e!==0||1/e===1/t)||e!==e&&t!==t}var x=typeof Object.is=="function"?Object.is:I,A=d.useState,F=d.useEffect,P=d.useLayoutEffect,W=d.useDebugValue;function z(e,t){var n=t(),o=A({inst:{value:n,getSnapshot:t}}),r=o[0].inst,u=o[1];return P(function(){r.value=n,r.getSnapshot=t,p(r)&&u({inst:r})},[e,n,t]),F(function(){return p(r)&&u({inst:r}),e(function(){p(r)&&u({inst:r})})},[e]),W(n),n}function p(e){var t=e.getSnapshot;e=e.value;try{var n=t();return!x(e,n)}catch{return!0}}function M(e,t){return t()}var _=typeof window>"u"||typeof window.document>"u"||typeof window.document.createElement>"u"?M:z;O.useSyncExternalStore=d.useSyncExternalStore!==void 0?d.useSyncExternalStore:_;w.exports=O;var C=w.exports;/**
 * @license React
 * use-sync-external-store-shim/with-selector.production.js
 *
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */var b=g,L=C;function T(e,t){return e===t&&(e!==0||1/e===1/t)||e!==e&&t!==t}var U=typeof Object.is=="function"?Object.is:T,B=L.useSyncExternalStore,q=b.useRef,G=b.useEffect,k=b.useMemo,H=b.useDebugValue;R.useSyncExternalStoreWithSelector=function(e,t,n,o,r){var u=q(null);if(u.current===null){var l={hasValue:!1,value:null};u.current=l}else l=u.current;u=k(function(){function E(i){if(!v){if(v=!0,s=i,i=o(i),r!==void 0&&l.hasValue){var c=l.value;if(r(c,i))return f=c}return f=i}if(c=f,U(s,i))return c;var y=o(i);return r!==void 0&&r(c,y)?(s=i,c):(s=i,f=y)}var v=!1,s,f,a=n===void 0?null:n;return[function(){return E(t())},a===null?void 0:function(){return E(a())}]},[t,n,o,r]);var S=B(e,u[0],u[1]);return G(function(){l.hasValue=!0,l.value=S},[S]),H(S),S};h.exports=R;var J=h.exports;const K=j(J),{useDebugValue:N}=V,{useSyncExternalStoreWithSelector:Q}=K;const X=e=>e;function Y(e,t=X,n){const o=Q(e.subscribe,e.getState,e.getServerState||e.getInitialState,t,n);return N(o),o}const D=e=>{const t=typeof e=="function"?$(e):e,n=(o,r)=>Y(t,o,r);return Object.assign(n,t),n},ee=e=>e?D(e):D;export{ee as c};
