/**
 * @file
 *
 * @brief Common header for many/most/all CUDA API wrapper example programs.
 */
#ifndef EXAMPLES_COMMON_HPP_
#define EXAMPLES_COMMON_HPP_


#include <string>
#include <iostream>
#ifdef __GNUC__
#include <cxxabi.h>
#endif

void report_current_context(const std::string& prefix);
void report_context_stack(const std::string& prefix);

#include <cuda/api.hpp>

#include <cstdio>
#include <fstream>
#include <cmath>
#include <cstring>
#include <system_error>
#include <memory>
#include <cstdlib>
#include <vector>
#include <algorithm>
#include <iomanip>
#include <numeric>

#if __GNUC__
template <typename T>
[[gnu::warning("type printed for your convenience")]]
bool your_type_was_() { return true; }

#define print_type_of(_x) your_type_was_<decltype(_x)>()
#endif

inline const char* ordinal_suffix(int n)
{
	static constexpr char suffixes [4][5] = {"th", "st", "nd", "rd"};
	auto ord = n % 100;
	if (ord / 10 == 1) { ord = 0; }
	ord = ord % 10;
	return suffixes[ord > 3 ? 0 : ord];
}

template <typename N = int>
inline ::std::string xth(N n) { return ::std::to_string(n) + ordinal_suffix(n); }

const char* cache_preference_name(cuda::multiprocessor_cache_preference_t pref)
{
	static const char* cache_preference_names[] = {
		"No preference",
		"Equal L1 and shared memory",
		"Prefer shared memory over L1",
		"Prefer L1 over shared memory",
	};
	return cache_preference_names[static_cast<off_t>(pref)];
}

const char* host_thread_sync_scheduling_policy_name(cuda::context::host_thread_sync_scheduling_policy_t policy)
{
	static const char *names[] = {
		"heuristic",
		"spin",
		"yield",
		"INVALID",
		"block",
		nullptr
	};
	return names[static_cast<off_t>(policy)];
}

const char* memory_type_name(cuda::memory::type_t mem_type)
{
	static const char* memory_type_names[] = {
		"N/A",
		"host",
		"device",
		"array",
		"unified"
	};
	return (mem_type <= 0 or mem_type > sizeof(memory_type_names)/sizeof(const char*)) ?
		 "invalid memory type": // or maybe we should die?
		 memory_type_names[mem_type];
}

namespace std {

std::ostream& operator<<(std::ostream& os, cuda::device::compute_capability_t cc)
{
    return os << cc.major() << '.' << cc.minor();
}

std::ostream& operator<<(std::ostream& os, cuda::multiprocessor_cache_preference_t pref)
{
	return os << cache_preference_name(pref);
}

std::ostream& operator<<(std::ostream& os, cuda::context::host_thread_sync_scheduling_policy_t pref)
{
	return os << host_thread_sync_scheduling_policy_name(pref);
}

std::ostream& operator<<(std::ostream& os, cuda::context::handle_t handle)
{
	return (os << cuda::detail_::ptr_as_hex(handle));
}

std::ostream& operator<<(std::ostream& os, const cuda::context_t& context)
{
	return os << "[device " << context.device_id() << " handle " << context.handle() << ']';
}

std::ostream& operator<<(std::ostream& os, const cuda::device_t& device)
{
	return os << cuda::device::detail_::identify(device.id());
}

std::ostream& operator<<(std::ostream& os, const cuda::stream_t& stream)
{
	return os << cuda::stream::detail_::identify(stream.handle(), stream.device().id());
}

std::ostream& operator<<(std::ostream& os, const cuda::launch_configuration_t& lc)
{
	return os << "launch config [ "
		<< "grid: ("  << lc.dimensions.grid.x << ", " << lc.dimensions.grid.y << ", " << lc.dimensions.grid.z << "), "
		<< "block: (" << lc.dimensions.block.x << ", " << lc.dimensions.block.y << ", " << lc.dimensions.block.z << "), "
		<< "dynamic shared mem: " << lc.dynamic_shared_memory_size
		<< "]";
}

std::ostream& operator<<(std::ostream& os, const cuda::grid::block_dimensions_t& dims)
{
	// return os << dims.x << "x" << dims.y << "x" << dims.z;
	return os << "(" << dims.x << ", " << dims.y << ", " << dims.z << ")";
}

std::ostream& operator<<(std::ostream& os, const cuda::memory::type_t mem_type)
{
	return os << memory_type_name(mem_type);
}

std::string to_string(const cuda::context_t& context)
{
	std::stringstream ss;
	ss.clear();
	ss << context;
	return ss.str();
}

} // namespace std

[[noreturn]] bool die_(const std::string& message)
{
	std::cerr << message << "\n";
	exit(EXIT_FAILURE);
}

#define assert_(cond) \
{ \
	auto evaluation_result = (cond); \
	if (not evaluation_result) \
		die_("Assertion failed at line " + std::to_string(__LINE__) + ": " #cond); \
}


void report_current_context(const std::string& prefix = "")
{
	if (not prefix.empty()) { std::cout << prefix << ", the current context is: "; }
	else std::cout << "The current context is: ";
	if (not cuda::context::current::exists()) {
		std::cout << "(None)" << std::endl;
	}
	else {
		auto cc = cuda::context::current::get();
		std::cout << cc << std::endl;
	}
}

void print_context_stack()
{
	if (not cuda::context::current::exists()) {
		std::cout << "(Context stack is empty/uninitialized)" << std::endl;
		return;
	}
	std::vector<cuda::context::handle_t> contexts;
	while(cuda::context::current::exists()) {
		contexts.push_back(cuda::context::current::detail_::pop());
	}
	for (auto handle : contexts) {
		auto device_id = cuda::context::detail_::get_device_id(handle);
		std::cout << handle << " for device " << device_id;
		if (cuda::context::detail_::is_primary(handle)) {
			std::cout << " (primary, "
				<< (cuda::device::primary_context::detail_::is_active(device_id) ? "active" : "inactive")
				<< ')';
		}
		std::cout << '\n';
	}
	for (auto it = contexts.rbegin(); it != contexts.rend(); ++it) {
		cuda::context::current::detail_::push(*it);
	}
}

void report_primary_context_activity(const std::string& prefix = "")
{
	if (not prefix.empty()) { std::cout << prefix << ", "; }
	std::cout << "Device primary contexts activity: ";
	for(auto device : cuda::devices()) {
		std::cout << device.id() << ": "
				  << (cuda::device::primary_context::detail_::is_active(device.id()) ? "ACTIVE" : "inactive")
				  << "  ";
	}
	std::cout << '\n';
}

void report_context_stack(const std::string& prefix = "")
{
	if (not prefix.empty()) { std::cout << prefix << ", the context stack is (top to bottom):\n"; }
	std::cout << "-----------------------------------------------------\n";
	print_context_stack();
	std::cout << "---\n";
	report_primary_context_activity();
	std::cout << "-----------------------------------------------------\n" << std::flush;
}

template <cuda::dimensionality_t Dimensionality>
std::ostream& operator<<(std::ostream& os, cuda::memory::copy_parameters_t<Dimensionality>& params);

std::ostream &stream_endpoint(
	std::ostream &os,
	const char* name,
	cuda::optional<cuda::context::handle_t > context,
	size_t xInBytes, size_t y, cuda::optional<size_t> z,
	CUmemorytype memoryType,
	CUarray array,
	const void* host,
	cuda::memory::device::address_t device,
	size_t pitch,
	cuda::optional<size_t> height)
{
	os << name << ": [ ";
	if (context) {
		os << "context: " << *context << " ";
	}
	os << "offset: ";
	if  (xInBytes == 0 and y == 0 and (not z or *z == 0)) { os << "(none)"; }
	else {
		os << '(' << xInBytes << " x " << y;
		if (z) { os << " x " << *z; }
		os << ")";
	}
	os << "; ";

	static const char* memory_type_names[] = { "(invalid type)", "host", "device", "array", "unified or host" };

	if (memoryType < (sizeof(memory_type_names) / sizeof(memory_type_names[0]))) {
		os << memory_type_names[memoryType] << " memory: ";
	}
	switch(memoryType) {
	case CU_MEMORYTYPE_ARRAY:
		os << "handle " << array;
		break;
	case CU_MEMORYTYPE_UNIFIED:
	case CU_MEMORYTYPE_HOST:
		os << host;
		break;
	case CU_MEMORYTYPE_DEVICE:
		os << device;
		break;
	default:
		os << "UNKNOWN TYPE!";
	}
	switch(memoryType) {
	case CU_MEMORYTYPE_UNIFIED:
	case CU_MEMORYTYPE_HOST:
	case CU_MEMORYTYPE_DEVICE:
		os << " with pitch " << pitch;
		if (height) { os << " and height " << *height; }
	default:
		break;
	}

	os << " ]";
	return os;
}

template <>
std::ostream& operator<< <2>(std::ostream& os, cuda::memory::copy_parameters_t<2>& params)
{
	auto flags = os.flags();
	os << "[ ";
	auto& p = params;
	stream_endpoint(os, "source", {}, p.srcXInBytes, p.srcY, {}, p.srcMemoryType, p.srcArray, p.srcHost, p.srcDevice, p.srcPitch, {});
	os << " ";
	stream_endpoint(os, "dest", {}, p.dstXInBytes, p.dstY, {}, p.dstMemoryType, p.dstArray, p.dstHost, p.dstDevice, p.dstPitch, {});

	os << " byte extents: " << p.WidthInBytes << " x " << p.Height << " ]";

	os.flags(flags);
	return os;
}

template <>
std::ostream& operator<< <3>(std::ostream& os, cuda::memory::copy_parameters_t<3>& params)
{
	auto flags = os.flags();
	os << "[ ";
	auto& p = params;
	stream_endpoint(os, "source", p.srcContext, p.srcXInBytes, p.srcY, p.srcZ, p.srcMemoryType, p.srcArray, p.srcHost, p.srcDevice, p.srcPitch, p.srcHeight);
	os << " ";
	stream_endpoint(os, "dest", p.dstContext, p.dstXInBytes, p.dstY, p.dstZ, p.dstMemoryType, p.dstArray, p.dstHost, p.dstDevice, p.dstPitch, p.srcHeight);

	os << " byte extents: " << p.WidthInBytes << " x " << p.Height << " x " << p.Depth << " ]";

	os.flags(flags);
	return os;
}


// Note: This will only work correctly for positive values
template <typename U1, typename U2>
typename std::common_type<U1,U2>::type div_rounding_up(U1 dividend, U2 divisor)
{
	return dividend / divisor + !!(dividend % divisor);
}

cuda::device::id_t choose_device(int argc, char const** argv)
{
	auto num_devices = cuda::device::count();
	if (num_devices == 0) {
		die_("No CUDA devices on this system");
	}

	auto device_id = [&]() -> cuda::device::id_t {
		if (argc == 1) {
			return cuda::device::default_device_id;
		}
		std::string device_id_arg { argv[1] };
		std::string prefix { "--device=" };
		if (device_id_arg.rfind(prefix) == 0) {
			device_id_arg = device_id_arg.substr(prefix.length());
		}
		return std::stoi(device_id_arg);
	}();

	if (device_id < 0) {
		die_("A negative device ID cannot be valid");
	}
	if (num_devices <= device_id) {
		die_("CUDA device " +  std::to_string(device_id) + " was requested, but there are only "
			+ std::to_string(num_devices) + " CUDA devices on this system");
	}
	std::cout << "Using CUDA device " << cuda::device::detail_::get_name(device_id) << " (having device ID " << device_id << ")\n";
	return device_id;
}

cuda::device::id_t choose_device(int argc, char ** argv)
{
	return choose_device(argc, const_cast<char const**>(argv));
}

#ifdef __GNUC__

inline char const* describe_demangling_status(int status)
{
	switch (status) {
	case 0: return "success";
	case 1: return "A memory allocation failure occurred";
	case 2: return "mangled_name is not a valid name under the C++ ABI mangling rules";
	case 3: return "One of the arguments is invalid";
	default: return "Unknown demangling status";
	}
}

// Inefficient, but simple
inline std::string demangle(const char *mangled_name)
{
	if (mangled_name == nullptr) { return nullptr; }
	int status;
	auto no_preallocated_output_buffer = nullptr;
	auto dont_return_mangled_length = nullptr;
	char *raw_demangled = abi::__cxa_demangle(
		mangled_name,
		no_preallocated_output_buffer,
		dont_return_mangled_length,
		&status);
	if (raw_demangled == nullptr) {
		throw std::runtime_error(std::string("Failed demangling \"") + mangled_name + "\": "
			+ describe_demangling_status(status));
	}
	std::string result { raw_demangled };
	free(raw_demangled);
	return result;
}
#endif


#endif // EXAMPLES_COMMON_HPP_
