/*
 * Original code Copyright (c) 2022, NVIDIA CORPORATION. All rights reserved.
 * Modifications Copyright (c) 2024, Eyal Rozenberg <eyalroz1@gmx.com>
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *  * Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 *  * Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 *  * Neither the name of NVIDIA CORPORATION nor the names of its
 *    contributors may be used to endorse or promote products derived
 *    from this software without specific prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS ``AS IS'' AND ANY
 * EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE COPYRIGHT OWNER OR
 * CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
 * EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
 * PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
 * PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY
 * OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
 * (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
 * OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
 */

/*
* Matrix multiplication: C = A * B.
* Host code.
*
* This sample implements matrix multiplication as described in Chapter 3
* of the programming guide and uses the CUBLAS library to demonstrate
* the best performance.

* SOME PRECAUTIONS:
* IF WE WANT TO CALCULATE ROW-MAJOR MATRIX MULTIPLY C = A * B,
* WE JUST NEED CALL CUBLAS API IN A REVERSE ORDER: cublasSegemm(B, A)!
* The reason is explained as follows:

* CUBLAS library uses column-major storage, but C/C++ use row-major storage.
* When passing the matrix pointer to CUBLAS, the memory layout alters from
* row-major to column-major, which is equivalent to an implicit transpose.

* In the case of row-major C/C++ matrix A, B, and a simple matrix multiplication
* C = A * B, we can't use the input order like cublasSgemm(A, B)  because of
* implicit transpose. The actual result of cublasSegemm(A, B) is A(T) * B(T).
* If col(A(T)) != row(B(T)), equal to row(A) != col(B), A(T) and B(T) are not
* multipliable. Moreover, even if A(T) and B(T) are multipliable, the result C
* is a column-based cublas matrix, which means C(T) in C/C++, we need extra
* transpose code to convert it to a row-based C/C++ matrix.

* To solve the problem, let's consider our desired result C, a row-major matrix.
* In cublas format, it is C(T) actually (because of the implicit transpose).
* C = A * B, so C(T) = (A * B) (T) = B(T) * A(T). Cublas matrice B(T) and A(T)
* happen to be C/C++ matrice B and A (still because of the implicit transpose)!
* We don't need extra transpose code, we only need alter the input order!
*
* CUBLAS provides high-performance matrix multiplication.
* See also:
* V. Volkov and J. Demmel, "Benchmarking GPUs to tune dense linear algebra,"
* in Proc. 2008 ACM/IEEE Conf. on Supercomputing (SC '08),
* Piscataway, NJ: IEEE Press, 2008, pp. Art. 31:1-11.
*/

#include <cublas_v2.h>
#include "../../common.hpp"


// Optional Command-line multiplier for matrix sizes
typedef struct _matrixSize {
	unsigned int uiWA, uiHA, uiWB, uiHB, uiWC, uiHC;
} sMatrixSize;

////////////////////////////////////////////////////////////////////////////////
//! Compute reference data set matrix multiply on CPU
//! C = A * B
//! @param C          reference data, computed but preallocated
//! @param A          matrix A as provided to device
//! @param B          matrix B as provided to device
//! @param hA         height of matrix A
//! @param wB         width of matrix B
////////////////////////////////////////////////////////////////////////////////
void matrixMulCPU(float *C, const float *A, const float *B, unsigned int hA,
				  unsigned int wA, unsigned int wB)
{
	for (unsigned int i = 0; i < hA; ++i)
		for (unsigned int j = 0; j < wB; ++j) {
			double sum = 0;

			for (unsigned int k = 0; k < wA; ++k) {
				double a = A[i * wA + k];
				double b = B[k * wB + j];
				sum += a * b;
			}

			C[i * wB + j] = (float) sum;
		}
}

inline bool compare_l2_norm(
	cuda::span<float const> reference,
	cuda::span<const float> data,
	float const epsilon)
{
	if (reference.size() != data.size()) {
		std::cerr << "Sizes of two spans to be compared - differ.";
		exit(EXIT_FAILURE);
	}
	assert_(epsilon >= 0);

	float error = 0;
	float ref = 0;

	for (unsigned int i = 0; i < data.size(); ++i) {
		float diff = reference[i] - data[i];
		error += diff * diff;
		ref += reference[i] * reference[i];
	}

	float normRef = ::sqrtf(ref);

	if (fabs(ref) < 1e-7) {
		std::cerr << "ERROR, reference l2-norm is 0\n";
		exit(EXIT_FAILURE);
	}

	float normError = ::sqrtf(error);
	error = normError / normRef;
	bool result = error < epsilon;
	if (not result) {
		std::cerr << "ERROR, L2-norm error " << error << " is greater than epsilon " << epsilon << "\n";
	}
	return result;
}

sMatrixSize initialize_matrix_dimensions()
{
	auto matrix_size_multiplier{5};
	sMatrixSize matrix_dims;
	int block_size{32};

	matrix_dims.uiWA = 3 * block_size * matrix_size_multiplier;
	matrix_dims.uiHA = 4 * block_size * matrix_size_multiplier;

	matrix_dims.uiWB = 2 * block_size * matrix_size_multiplier;
	matrix_dims.uiHB = 3 * block_size * matrix_size_multiplier;

	matrix_dims.uiWC = 2 * block_size * matrix_size_multiplier;
	matrix_dims.uiHC = 4 * block_size * matrix_size_multiplier;

	std::cout
		<< "MatrixA(" << matrix_dims.uiHA << ',' << matrix_dims.uiWA << "), "
		<< "MatrixB(" << matrix_dims.uiHB << ',' << matrix_dims.uiWB << "), "
		<< "MatrixC(" << matrix_dims.uiHC << ',' << matrix_dims.uiWC << ")\n";

	if (matrix_dims.uiWA != matrix_dims.uiHB ||
		matrix_dims.uiHA != matrix_dims.uiHC ||
		matrix_dims.uiWB != matrix_dims.uiWC) {
		printf("ERROR: Matrix sizes do not match!\n");
		exit(EXIT_FAILURE);
	}
	return matrix_dims;
}

void multiply_and_time_with_cublas(
	cuda::device_t device,
	cuda::span<float> d_A,
	cuda::span<float> d_B,
	cuda::span<float> d_C,
	cuda::span<float> h_CUBLAS,
	sMatrixSize matrix_dims,
	int num_iterations)
{
	std::cout << "Computing result using CUBLAS... ";

	const float alpha = 1.0f;
	const float beta = 0.0f;
	cublasHandle_t handle;

	cublasCreate(&handle);

	// Perform warmup operation with cublas
	cublasSgemm(
		handle, CUBLAS_OP_N, CUBLAS_OP_N,
		matrix_dims.uiWB, matrix_dims.uiHA, matrix_dims.uiWA, // m, n, k
		&alpha, d_B.data(),
		matrix_dims.uiWB, // lda
		d_A.data(),
		matrix_dims.uiWA, // ldb
		&beta,
		d_C.data(),
		matrix_dims.uiWB // ldc
	);

	// Allocate CUDA events that we'll use for timing

	// Record the start event
	auto stream = device.default_stream();
	auto start = stream.enqueue.event();

	for (int iteration_index = 0; iteration_index < num_iterations; iteration_index++) {
		// note cublas is column primary!
		// need to transpose the order
		cublasSgemm(
			handle, CUBLAS_OP_N, CUBLAS_OP_N, matrix_dims.uiWB, matrix_dims.uiHA,
			matrix_dims.uiWA, &alpha, d_B.data(), matrix_dims.uiWB, d_A.data(),
			matrix_dims.uiWA, &beta, d_C.data(), matrix_dims.uiWB);
	}
	auto end = stream.enqueue.event();

	std::cout << "done.\n";

	// Wait for the stop event to complete
	end.synchronize();

	auto total = cuda::event::time_elapsed_between(start, end);

	// Compute and print the performance
	auto msec_per_iteration = total.count() / (float) num_iterations;
	double ops_per_multiplication = 2.0 * (double) matrix_dims.uiHC *
									(double) matrix_dims.uiWC *
									(double) matrix_dims.uiHB;
	double giga_ops_per_second =
		(ops_per_multiplication * 1.0e-9f) / (msec_per_iteration / 1000.0f);
	printf("Performance= %.2f GFlop/s, Time= %.3f msec, Size= %.0f Ops\n",
		giga_ops_per_second, msec_per_iteration, ops_per_multiplication);

	cuda::memory::copy(h_CUBLAS, d_C);

	// Destroy the handle
	cublasDestroy(handle);
}

////////////////////////////////////////////////////////////////////////////////
//! Run a simple test matrix multiply using CUBLAS
////////////////////////////////////////////////////////////////////////////////

int main(int argc, char **argv)
{
	std::cout << "[Matrix Multiply CUBLAS] - Starting...\n";
	auto device_id = choose_device(argc, argv);
	auto device = cuda::device::get(device_id);

	std::cout << "GPU Device " << device_id << ": \"" << device.name() << "\" "
			  << "with compute capability " << device.compute_capability() << '\n';

	auto matrix_dims = initialize_matrix_dimensions();
	int num_iterations = 30;

	auto size_A = matrix_dims.uiWA * matrix_dims.uiHA;
	auto size_B = matrix_dims.uiWB * matrix_dims.uiHB;
	auto size_C = matrix_dims.uiWC * matrix_dims.uiHC;

	auto h_A = cuda::make_unique_span<float>(size_A);
	auto h_B = cuda::make_unique_span<float>(size_B);
	auto h_CUBLAS_result = cuda::make_unique_span<float>(size_C);

	// set seed for rand()
	srand(2006);

	// initialize host memory
	auto generator = []() { return static_cast<float>(rand()) / static_cast<float>(RAND_MAX); };
	std::generate(h_A.begin(), h_A.end(), generator);
	std::generate(h_B.begin(), h_B.end(), generator);

	// allocate device memory
	auto d_A = cuda::memory::make_unique_span<float>(device, size_A);
	auto d_B = cuda::memory::make_unique_span<float>(device, size_B);
	auto d_C = cuda::memory::make_unique_span<float>(device, size_C);

	cuda::memory::copy(d_A, h_A);
	cuda::memory::copy(d_B, h_B);

	multiply_and_time_with_cublas(device, d_A, d_B, d_C, h_CUBLAS_result, matrix_dims, num_iterations);

	// compute reference solution
	std::cout << "Computing result using host CPU... ";
	auto h_CPU_result = cuda::make_unique_span<float>(size_C);
	matrixMulCPU(h_CPU_result.data(), h_A.data(), h_B.data(), matrix_dims.uiHA, matrix_dims.uiWA, matrix_dims.uiWB);
	std::cout << "done.\n";

	bool about_equal = compare_l2_norm(h_CPU_result, h_CUBLAS_result, 1.0e-6f);

	std::cout << "CUBLAS Matrix Multiply is close enough to CPU results: " << (about_equal ? "Yes" : "No") << '\n';
	std::cout << (about_equal ? "SUCCESS" : "FAILURE") << '\n';
}
